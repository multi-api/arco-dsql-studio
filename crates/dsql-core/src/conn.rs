//! TLS connections to Aurora DSQL with IAM tokens, `search_path`, cancellation and a small recycling pool.

use std::ops::Deref;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use aws_config::SdkConfig;
use serde::{Deserialize, Serialize};
use tokio::sync::{OnceCell, OwnedSemaphorePermit, Semaphore};
use tokio::task::AbortHandle;
use tokio_postgres::config::SslMode;
use tokio_postgres::{CancelToken, Client};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::error::CoreError;
use crate::identity::sdk_config;
use crate::quote::ident;
use crate::token::{DbRole, auth_token};

/// DSQL closes connections after 60 minutes; the pool retires them first.
pub const RECYCLE_AFTER: Duration = Duration::from_secs(55 * 60);

/// How long `Session::cancel` waits for the cancel request before closing the connection anyway.
const CANCEL_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

pub fn is_too_old(opened: Instant, now: Instant) -> bool {
    now.duration_since(opened) >= RECYCLE_AFTER
}

pub fn set_search_path_sql(schema: &str) -> String {
    format!("SET search_path = {}", ident(schema))
}

/// Where and as whom to connect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub profile: String,
    pub region: String,
    pub host: String,
    pub role: DbRole,
    pub schema: Option<String>,
}

/// One open connection.
pub struct Session {
    pub client: Client,
    cancel_token: CancelToken,
    connection: AbortHandle,
    pub opened_at: Instant,
}

impl Session {
    /// Stops the running statement. DSQL ignores Postgres cancel requests (Task 1 spike), so after sending one the
    /// session closes its connection: the statement's future fails at once, and the pool replaces the connection. The
    /// statement may still finish on the server.
    pub async fn cancel(&self) {
        self.canceller().cancel().await;
    }

    /// A handle that cancels this session's running statement from elsewhere, for example another Tauri command.
    pub fn canceller(&self) -> Canceller {
        Canceller {
            cancel_token: self.cancel_token.clone(),
            connection: self.connection.clone(),
        }
    }

    /// Closed by the server, the network, `cancel`, or DSQL's 60-minute limit.
    pub fn is_closed(&self) -> bool {
        self.client.is_closed()
    }
}

/// See `Session::cancel`.
#[derive(Clone)]
pub struct Canceller {
    cancel_token: CancelToken,
    connection: AbortHandle,
}

impl Canceller {
    pub async fn cancel(&self) {
        let _ = tokio::time::timeout(
            CANCEL_REQUEST_TIMEOUT,
            self.cancel_token.cancel_query(tls()),
        )
        .await;
        self.connection.abort();
    }
}

/// rustls with the public web roots (DSQL's certificate chains to one). rustls always checks the chain and the
/// hostname, so `SslMode::Require` with this connector is `verify-full`. The provider is set explicitly because the
/// AWS SDK compiles in a second one.
fn tls() -> MakeRustlsConnect {
    static TLS: OnceLock<MakeRustlsConnect> = OnceLock::new();
    TLS.get_or_init(|| {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
        MakeRustlsConnect::new(config)
    })
    .clone()
}

/// Opens a connection with a fresh token, loading the profile's credentials first.
pub async fn connect(target: &Target) -> Result<Session, CoreError> {
    let cfg = sdk_config(&target.profile, &target.region).await?;
    connect_with(&cfg, target).await
}

/// Opens a connection with credentials already loaded: `dbname=postgres`, TLS verify-full, then `SET search_path`.
pub async fn connect_with(cfg: &SdkConfig, target: &Target) -> Result<Session, CoreError> {
    let token = auth_token(cfg, &target.host, &target.region, &target.role).await?;
    let mut pg = tokio_postgres::Config::new();
    pg.host(&target.host)
        .port(5432)
        .dbname("postgres")
        .user(target.role.user())
        .password(token)
        .ssl_mode(SslMode::Require)
        .application_name("arco-dsql-studio")
        .connect_timeout(Duration::from_secs(15));
    let (client, connection) = pg.connect(tls()).await.map_err(db_error)?;
    let connection = tokio::spawn(async move {
        let _ = connection.await;
    })
    .abort_handle();
    let session = Session {
        cancel_token: client.cancel_token(),
        client,
        connection,
        opened_at: Instant::now(),
    };
    if let Some(schema) = &target.schema {
        session
            .client
            .batch_execute(&set_search_path_sql(schema))
            .await
            .map_err(db_error)?;
    }
    Ok(session)
}

/// Postgres errors keep their SQLSTATE, detail and hint; connection failures have no SQLSTATE.
pub fn db_error(err: tokio_postgres::Error) -> CoreError {
    match err.as_db_error() {
        Some(db) => CoreError::Db {
            sqlstate: Some(db.code().code().to_string()),
            message: db.message().to_string(),
            detail: db.detail().map(str::to_string),
            hint: db.hint().map(str::to_string),
        },
        None => CoreError::Db {
            sqlstate: None,
            message: err.to_string(),
            detail: None,
            hint: None,
        },
    }
}

/// A few connections to one target, opened on demand with fresh tokens. Sessions older than `RECYCLE_AFTER` or closed
/// are dropped instead of reused, so a long-lived app never hits DSQL's 60-minute limit mid-statement.
#[derive(Clone)]
pub struct Pool {
    inner: Arc<PoolInner>,
}

struct PoolInner {
    target: Target,
    slots: Arc<Semaphore>,
    idle: Mutex<Vec<Session>>,
    cfg: OnceCell<SdkConfig>,
}

impl Pool {
    pub fn new(target: Target, max: usize) -> Pool {
        Pool {
            inner: Arc::new(PoolInner {
                target,
                slots: Arc::new(Semaphore::new(max.max(1))),
                idle: Mutex::new(Vec::new()),
                cfg: OnceCell::new(),
            }),
        }
    }

    pub fn target(&self) -> &Target {
        &self.inner.target
    }

    /// Waits for a free slot, then reuses a healthy idle session or opens a new one.
    pub async fn get(&self) -> Result<PooledSession, CoreError> {
        let permit = self
            .inner
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| CoreError::Cancelled)?;
        let reusable = {
            let mut idle = self.inner.idle.lock().expect("pool lock");
            let now = Instant::now();
            idle.retain(|s| !s.is_closed() && !is_too_old(s.opened_at, now));
            idle.pop()
        };
        let session = match reusable {
            Some(session) => session,
            None => {
                let cfg = self
                    .inner
                    .cfg
                    .get_or_try_init(|| {
                        sdk_config(&self.inner.target.profile, &self.inner.target.region)
                    })
                    .await?;
                connect_with(cfg, &self.inner.target).await?
            }
        };
        Ok(PooledSession {
            session: Some(session),
            pool: self.inner.clone(),
            _permit: permit,
            discard: false,
        })
    }

    /// Closes every idle session, for example after the schema changed.
    pub fn clear(&self) {
        self.inner.idle.lock().expect("pool lock").clear();
    }
}

/// A session borrowed from the pool; it goes back when dropped unless it is closed, too old, or discarded.
pub struct PooledSession {
    session: Option<Session>,
    pool: Arc<PoolInner>,
    _permit: OwnedSemaphorePermit,
    discard: bool,
}

impl PooledSession {
    /// Do not return this session to the pool (for example, a transaction may still be open on it).
    pub fn discard(&mut self) {
        self.discard = true;
    }
}

impl Deref for PooledSession {
    type Target = Session;

    fn deref(&self) -> &Session {
        self.session.as_ref().expect("session present until drop")
    }
}

impl Drop for PooledSession {
    fn drop(&mut self) {
        if let Some(session) = self.session.take()
            && !self.discard
            && !session.is_closed()
            && !is_too_old(session.opened_at, Instant::now())
        {
            self.pool.idle.lock().expect("pool lock").push(session);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_recycles_old_connections() {
        let opened = Instant::now();
        assert!(!is_too_old(opened, opened + Duration::from_secs(54 * 60)));
        assert!(!is_too_old(
            opened,
            opened + Duration::from_secs(55 * 60 - 1)
        ));
        assert!(is_too_old(opened, opened + Duration::from_secs(55 * 60)));
        assert!(is_too_old(opened, opened + Duration::from_secs(61 * 60)));
    }

    #[test]
    fn search_path_statement_quotes() {
        assert_eq!(
            set_search_path_sql("My Schema"),
            "SET search_path = \"My Schema\""
        );
        assert_eq!(
            set_search_path_sql("app_staging"),
            "SET search_path = \"app_staging\""
        );
        assert_eq!(
            set_search_path_sql("x\"; DROP TABLE t; --"),
            "SET search_path = \"x\"\"; DROP TABLE t; --\""
        );
    }
}
