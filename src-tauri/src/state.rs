//! What the app keeps between commands: pending SSO sign-ins, open connections and the local store. Credentials,
//! SSO tokens and DSQL tokens stay here and never go to the frontend.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use dsql_core::CoreError;
use dsql_core::conn::{Canceller, Pool, Target};
use dsql_core::sso::{DeviceLogin, SsoToken};
use dsql_core::store::{SavedConnection, Store};
use dsql_core::token::DbRole;

/// Connections per open connection's pool: the editor, the table browser and a background catalog read.
pub const POOL_SIZE: usize = 4;

pub struct AppState {
    next_id: AtomicU64,
    pub logins: Mutex<HashMap<String, PendingLogin>>,
    pub connections: Mutex<HashMap<String, Arc<OpenConnection>>>,
    pub store: Mutex<Store>,
}

/// An SSO sign-in between "start" and "profile saved". The token is kept only until the profile is written, when it
/// goes to the AWS CLI's cache like `aws sso login` would.
pub struct PendingLogin {
    pub start_url: String,
    pub sso_region: String,
    pub device: DeviceLogin,
    pub token: Option<SsoToken>,
}

pub struct OpenConnection {
    pub saved: SavedConnection,
    /// Replaced when the user switches schema, since every pooled session carries its `search_path`.
    pub pool: Mutex<Pool>,
    /// The statement running now, if any, so `cancel` can stop it.
    pub running: Mutex<Option<Canceller>>,
}

impl OpenConnection {
    pub fn pool(&self) -> Pool {
        self.pool.lock().expect("pool lock").clone()
    }

    pub fn schema(&self) -> Option<String> {
        self.pool().target().schema.clone()
    }
}

impl AppState {
    pub fn new(store: Store) -> AppState {
        AppState {
            next_id: AtomicU64::new(1),
            logins: Mutex::new(HashMap::new()),
            connections: Mutex::new(HashMap::new()),
            store: Mutex::new(store),
        }
    }

    pub fn new_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    pub fn connection(&self, id: &str) -> Result<Arc<OpenConnection>, CoreError> {
        self.connections
            .lock()
            .expect("connections lock")
            .get(id)
            .cloned()
            .ok_or_else(|| CoreError::Config(format!("connection {id} is not open")))
    }

    pub fn with_store<T>(
        &self,
        f: impl FnOnce(&Store) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        f(&self.store.lock().expect("store lock"))
    }
}

pub fn db_role(role: &str) -> DbRole {
    if role == "admin" {
        DbRole::Admin
    } else {
        DbRole::Custom(role.to_string())
    }
}

pub fn target(saved: &SavedConnection, schema: Option<String>) -> Target {
    Target {
        profile: saved.profile.clone(),
        region: saved.region.clone(),
        host: saved.host.clone(),
        role: db_role(&saved.role),
        schema,
    }
}
