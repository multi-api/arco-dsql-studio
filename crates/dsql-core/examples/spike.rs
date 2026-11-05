//! Task 1 spike: answers the open questions of spec section 8 against a real Aurora DSQL cluster.
//!
//! It never uses an Arco app's tables. `setup` builds a stand-in for one on a scratch cluster: a role `spike_owner`
//! that owns the tables of schema `spike_app`, so `admin` hits the same walls as with an Arco app. `teardown` removes
//! everything `setup` and `run` created.
//!
//! Environment (never commit real values):
//! - `DSQL_HOST`, `DSQL_REGION`, `AWS_PROFILE`
//! - `SPIKE_IAM_ROLE_ARN`: the IAM role ARN the spike roles are mapped to (the role behind `AWS_PROFILE`)
//!
//! Usage: `cargo run -p dsql-core --example spike -- setup|run|teardown`

use std::time::{Duration, Instant};

use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_sdk_dsql::auth_token::{AuthTokenGenerator, Config as TokenConfig};
use tokio_postgres::{Client, SimpleQueryMessage};
use tokio_postgres_rustls::MakeRustlsConnect;

const SCHEMA: &str = "spike_app";
const OWNER: &str = "spike_owner";
const READER: &str = "spike_readonly";

type BoxError = Box<dyn std::error::Error + Send + Sync>;

struct Env {
    host: String,
    region: String,
    iam_role_arn: String,
    sdk: SdkConfig,
    tls: MakeRustlsConnect,
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cmd = std::env::args().nth(1).unwrap_or_default();
    let env = load_env().await?;

    match cmd.as_str() {
        "setup" => setup(&env).await,
        "run" => run(&env).await,
        "teardown" => teardown(&env).await,
        "index" => step7_index(&env).await,
        _ => {
            eprintln!("usage: spike setup|run|teardown");
            Ok(())
        }
    }
}

async fn load_env() -> Result<Env, BoxError> {
    let var = |k: &str| std::env::var(k).map_err(|_| format!("{k} is not set"));
    let host = var("DSQL_HOST")?;
    let region = var("DSQL_REGION")?;
    let iam_role_arn = var("SPIKE_IAM_ROLE_ARN")?;
    let sdk = aws_config::defaults(BehaviorVersion::latest())
        .profile_name(var("AWS_PROFILE")?)
        .region(Region::new(region.clone()))
        .load()
        .await;

    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    Ok(Env { host, region, iam_role_arn, sdk, tls: MakeRustlsConnect::new(tls_config) })
}

async fn token(env: &Env, role: &str) -> Result<String, BoxError> {
    let generator = AuthTokenGenerator::new(
        TokenConfig::builder()
            .hostname(env.host.clone())
            .region(Region::new(env.region.clone()))
            .expires_in(900)
            .build()?,
    );
    let token = if role == "admin" {
        generator.db_connect_admin_auth_token(&env.sdk).await?
    } else {
        generator.db_connect_auth_token(&env.sdk).await?
    };
    Ok(token.as_str().to_string())
}

/// Connects over TLS. rustls always checks the certificate chain and the hostname, which is `verify-full`.
async fn connect(env: &Env, role: &str) -> Result<Client, BoxError> {
    let mut cfg = tokio_postgres::Config::new();
    cfg.host(&env.host)
        .port(5432)
        .dbname("postgres")
        .user(role)
        .password(token(env, role).await?)
        .ssl_mode(tokio_postgres::config::SslMode::Require);
    let (client, conn) = cfg.connect(env.tls.clone()).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("    [connection closed: {e}]");
        }
    });
    Ok(client)
}

fn describe(err: &tokio_postgres::Error) -> String {
    match err.as_db_error() {
        Some(db) => format!("ERROR {} {}", db.code().code(), db.message()),
        None => format!("ERROR (no SQLSTATE) {err}"),
    }
}

/// Runs one statement with the simple query protocol and prints the outcome. Returns true on success.
async fn sql(client: &Client, label: &str, stmt: &str) -> bool {
    let started = Instant::now();
    match client.simple_query(stmt).await {
        Ok(msgs) => {
            let rows: Vec<Vec<Option<String>>> = msgs
                .iter()
                .filter_map(|m| match m {
                    SimpleQueryMessage::Row(r) => {
                        Some((0..r.len()).map(|i| r.get(i).map(str::to_string)).collect())
                    }
                    _ => None,
                })
                .collect();
            let shown: Vec<_> = rows.iter().take(5).collect();
            println!("  [{label}] OK in {} ms: {stmt}  -> {shown:?}", started.elapsed().as_millis());
            true
        }
        Err(e) => {
            println!("  [{label}] {} in {} ms: {stmt}", describe(&e), started.elapsed().as_millis());
            false
        }
    }
}

async fn setup(env: &Env) -> Result<(), BoxError> {
    println!("== setup: owner role, schema and tables (stand-in for an Arco app)");
    let admin = connect(env, "admin").await?;
    sql(&admin, "admin", &format!("CREATE ROLE {OWNER} WITH LOGIN")).await;
    sql(&admin, "admin", &format!("AWS IAM GRANT {OWNER} TO '{}'", env.iam_role_arn)).await;
    sql(&admin, "admin", &format!("CREATE SCHEMA {SCHEMA}")).await;
    sql(&admin, "admin", &format!("GRANT USAGE, CREATE ON SCHEMA {SCHEMA} TO {OWNER}")).await;

    let owner = connect(env, OWNER).await?;
    sql(&owner, OWNER, &format!("SET search_path = {SCHEMA}")).await;
    sql(&owner, OWNER, "CREATE TABLE users (id bigint PRIMARY KEY, name text NOT NULL, email text)").await;
    sql(
        &owner,
        OWNER,
        "CREATE TABLE orders (id bigint PRIMARY KEY, user_id bigint NOT NULL, total numeric(10,2), note text)",
    )
    .await;
    sql(&owner, OWNER, r#"CREATE TABLE "Order Items" ("we""ird" text, qty int)"#).await;
    sql(
        &owner,
        OWNER,
        "INSERT INTO users VALUES (1, 'Ana', 'ana@example.com'), (2, 'Ben', NULL), (3, 'NULL', ''), (4, 'O''Neil', 'o@example.com')",
    )
    .await;
    sql(&owner, OWNER, "INSERT INTO orders VALUES (1, 1, 10.50, 'first'), (2, 1, 3.00, NULL), (3, 2, 99.99, '')").await;
    sql(&owner, OWNER, r#"INSERT INTO "Order Items" VALUES ('a', 1)"#).await;
    Ok(())
}

async fn run(env: &Env) -> Result<(), BoxError> {
    println!("== step 1: connect as admin over TLS (hostname verified)");
    let admin = connect(env, "admin").await?;
    sql(&admin, "1", "select current_user, version()").await;
    println!("  verify-full check: connecting with an IP address instead of the hostname must fail");
    {
        let mut cfg = tokio_postgres::Config::new();
        let addr = tokio::net::lookup_host((env.host.as_str(), 5432)).await?.next().unwrap();
        cfg.host(&addr.ip().to_string())
            .port(5432)
            .dbname("postgres")
            .user("admin")
            .password(token(env, "admin").await?)
            .ssl_mode(tokio_postgres::config::SslMode::Require);
        match cfg.connect(env.tls.clone()).await {
            Ok(_) => println!("  [1] UNEXPECTED: connected by IP"),
            Err(e) => println!("  [1] by IP refused as expected: {e}"),
        }
    }
    println!("  baseline walls as admin:");
    sql(&admin, "1", "select count(*) from users").await;
    sql(&admin, "1", &format!("SET search_path = {SCHEMA}")).await;
    sql(&admin, "1", "select count(*) from users").await;
    sql(&admin, "1", "RESET search_path").await;
    drop(admin);

    step2(env).await?;
    step3(env).await?;
    step4(env).await?;
    step5_and_6(env).await?;
    step7_index(env).await?;
    Ok(())
}

async fn step7_index(env: &Env) -> Result<(), BoxError> {
    println!("== step 7: CREATE INDEX without ASYNC, on an empty and a non-empty table (scratch table in public)");
    let c = connect(env, "admin").await?;
    sql(&c, "7", "CREATE TABLE public.spike_idx (id int PRIMARY KEY, v text)").await;
    sql(&c, "7", "CREATE INDEX spike_idx_v_empty ON public.spike_idx (v)").await;
    sql(&c, "7", "INSERT INTO public.spike_idx VALUES (1, 'a'), (2, 'b')").await;
    sql(&c, "7", "CREATE INDEX spike_idx_v_rows ON public.spike_idx (v)").await;
    sql(&c, "7", "CREATE INDEX ASYNC spike_idx_v_async ON public.spike_idx (v)").await;
    sql(&c, "7", "TRUNCATE public.spike_idx").await;
    sql(&c, "7", "BEGIN").await;
    sql(&c, "7", "SAVEPOINT s").await;
    sql(&c, "7", "ROLLBACK").await;
    sql(&c, "7", "DROP TABLE public.spike_idx").await;
    sql(&c, "7", "select count(*) from pg_class where relname like 'spike_idx%'").await;
    Ok(())
}

async fn step2(env: &Env) -> Result<(), BoxError> {
    println!("== step 2: extended protocol inside transactions");
    let c = connect(env, "admin").await?;

    println!("  2a: query() (named prepared statement, closed when dropped), then select 1, commit");
    sql(&c, "2a", "BEGIN").await;
    match c.query("select $1::int as v", &[&1i32]).await {
        Ok(rows) => println!("  [2a] query OK: {}", rows[0].get::<_, i32>(0)),
        Err(e) => println!("  [2a] query {}", describe(&e)),
    }
    sql(&c, "2a", "select 1").await;
    sql(&c, "2a", "COMMIT").await;

    println!("  2b: prepare() + drop of the Statement (explicit Close), then select 1, commit");
    sql(&c, "2b", "BEGIN").await;
    match c.prepare("select 2 as v").await {
        Ok(stmt) => {
            let r = c.query(&stmt, &[]).await;
            println!("  [2b] prepared query: {:?}", r.as_ref().map(|r| r.len()).map_err(describe));
            drop(stmt);
        }
        Err(e) => println!("  [2b] prepare {}", describe(&e)),
    }
    // The Close message is sent lazily with the next request.
    sql(&c, "2b", "select 1").await;
    sql(&c, "2b", "select 1 -- second").await;
    sql(&c, "2b", "COMMIT").await;

    println!("  2c: query_typed() (unnamed statement), then select 1, commit");
    sql(&c, "2c", "BEGIN").await;
    match c
        .query_typed("select $1::text as v", &[(&"x", tokio_postgres::types::Type::TEXT)])
        .await
    {
        Ok(rows) => println!("  [2c] query_typed OK: {}", rows[0].get::<_, String>(0)),
        Err(e) => println!("  [2c] query_typed {}", describe(&e)),
    }
    sql(&c, "2c", "select 1").await;
    sql(&c, "2c", "COMMIT").await;

    println!("  2d: simple_query multi-statement, NULL vs empty vs 'NULL'");
    sql(&c, "2d", "select NULL::text as a, ''::text as b, 'NULL'::text as c; select 2").await;

    println!("  2e: an error inside a transaction, then the next statement");
    sql(&c, "2e", "BEGIN").await;
    sql(&c, "2e", "select 1/0").await;
    sql(&c, "2e", "select 1").await;
    sql(&c, "2e", "ROLLBACK").await;
    Ok(())
}

async fn step3(env: &Env) -> Result<(), BoxError> {
    println!("== step 3: cancelling select pg_sleep(30) after 2 s");
    let c = connect(env, "admin").await?;
    let cancel = c.cancel_token();
    let started = Instant::now();
    let tls = env.tls.clone();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let r = cancel.cancel_query(tls).await;
        println!("  [3] cancel_query sent: {:?}", r.map_err(|e| e.to_string()));
    });
    match c.simple_query("select pg_sleep(30)").await {
        Ok(_) => println!("  [3] query finished normally after {} ms (cancel ignored)", started.elapsed().as_millis()),
        Err(e) => println!("  [3] query ended after {} ms: {}", started.elapsed().as_millis(), describe(&e)),
    }
    canceller.await?;
    sql(&c, "3", "select 'connection still usable'").await;
    Ok(())
}

async fn step4(env: &Env) -> Result<(), BoxError> {
    println!("== step 4: catalog queries for the schema tree (as admin)");
    let c = connect(env, "admin").await?;
    let queries = [
        (
            "schemas",
            "select n.nspname, pg_get_userbyid(n.nspowner) from pg_namespace n \
             where n.nspname not in ('pg_catalog','information_schema','sys','pg_toast') \
             and n.nspname not like 'pg_temp_%' and n.nspname not like 'pg_toast_temp_%' order by 1"
                .to_string(),
        ),
        (
            "tables",
            format!(
                "select c.relname, c.relkind, pg_get_userbyid(c.relowner), \
                 has_table_privilege(c.oid, 'SELECT'), has_table_privilege(c.oid, 'INSERT'), \
                 has_table_privilege(c.oid, 'UPDATE'), has_table_privilege(c.oid, 'DELETE') \
                 from pg_class c join pg_namespace n on n.oid = c.relnamespace \
                 where n.nspname = '{SCHEMA}' and c.relkind in ('r','v','m','S','p') order by 1"
            ),
        ),
        (
            "columns",
            format!(
                "select a.attname, format_type(a.atttypid, a.atttypmod), not a.attnotnull, \
                 pg_get_expr(d.adbin, d.adrelid), a.attnum \
                 from pg_attribute a join pg_class c on c.oid = a.attrelid \
                 join pg_namespace n on n.oid = c.relnamespace \
                 left join pg_attrdef d on d.adrelid = a.attrelid and d.adnum = a.attnum \
                 where n.nspname = '{SCHEMA}' and c.relname = 'users' and a.attnum > 0 and not a.attisdropped \
                 order by a.attnum"
            ),
        ),
        (
            "indexes",
            format!(
                "select i.relname, ix.indisunique, ix.indisprimary, \
                 array(select a.attname from unnest(ix.indkey) with ordinality k(attnum, ord) \
                 join pg_attribute a on a.attrelid = ix.indrelid and a.attnum = k.attnum order by k.ord) \
                 from pg_index ix join pg_class i on i.oid = ix.indexrelid \
                 join pg_class t on t.oid = ix.indrelid join pg_namespace n on n.oid = t.relnamespace \
                 where n.nspname = '{SCHEMA}' and t.relname = 'users'"
            ),
        ),
        (
            "primary_key",
            format!(
                "select a.attname from pg_index ix \
                 join pg_attribute a on a.attrelid = ix.indrelid and a.attnum = any(ix.indkey) \
                 where ix.indrelid = '{SCHEMA}.users'::regclass and ix.indisprimary"
            ),
        ),
        (
            "information_schema.table_privileges",
            format!(
                "select table_name, grantee, privilege_type from information_schema.table_privileges \
                 where table_schema = '{SCHEMA}' order by 1, 2, 3"
            ),
        ),
        (
            "primary_key (key columns only: indnkeyatts)",
            format!(
                "select ix.indnatts, ix.indnkeyatts, \
                 array(select a.attname from unnest(ix.indkey) with ordinality k(attnum, ord) \
                 join pg_attribute a on a.attrelid = ix.indrelid and a.attnum = k.attnum \
                 where k.ord <= ix.indnkeyatts order by k.ord) \
                 from pg_index ix where ix.indrelid = '{SCHEMA}.users'::regclass and ix.indisprimary"
            ),
        ),
        (
            "has_table_privilege for another role",
            format!(
                "select c.relname, has_table_privilege('{OWNER}', c.oid, 'SELECT') \
                 from pg_class c join pg_namespace n on n.oid = c.relnamespace \
                 where n.nspname = '{SCHEMA}' and c.relkind = 'r' order by 1"
            ),
        ),
        ("role search_path default", "select rolname, rolconfig from pg_roles where rolname in ('admin','spike_owner','spike_readonly')".to_string()),
        ("current_schema", "select current_schema(), current_setting('search_path')".to_string()),
        ("iam mappings", "select * from sys.iam_pg_role_mappings".to_string()),
    ];
    for (label, q) in queries {
        sql(&c, label, &q).await;
    }
    Ok(())
}

async fn step5_and_6(env: &Env) -> Result<(), BoxError> {
    let admin = connect(env, "admin").await?;
    println!("== step 6a: create {READER} with the spec 6.5 recipe (admin part)");
    sql(&admin, "6", &format!("CREATE ROLE {READER} WITH LOGIN")).await;
    sql(&admin, "6", &format!("AWS IAM GRANT {READER} TO '{}'", env.iam_role_arn)).await;
    sql(&admin, "6", &format!("GRANT USAGE ON SCHEMA {SCHEMA} TO {READER}")).await;
    sql(&admin, "6", &format!("ALTER ROLE {READER} SET search_path = {SCHEMA}")).await;

    println!("== step 5: can admin take the owner's privileges? (GRANT owner TO admin; SET ROLE)");
    if sql(&admin, "5", &format!("GRANT {OWNER} TO admin")).await {
        sql(&admin, "5", &format!("SET ROLE {OWNER}")).await;
        sql(&admin, "5", "select current_user, session_user").await;
        sql(&admin, "5", &format!("select count(*) from {SCHEMA}.users")).await;
        sql(&admin, "5", &format!("GRANT SELECT ON {SCHEMA}.orders TO {READER}")).await;
        sql(&admin, "5", "RESET ROLE").await;
        sql(&admin, "5", &format!("REVOKE {OWNER} FROM admin")).await;
    }
    sql(&admin, "5", "select pg_has_role('admin', 'spike_owner', 'MEMBER')").await;
    println!("  after REVOKE, on a fresh admin connection:");
    let fresh = connect(env, "admin").await?;
    sql(&fresh, "5", "select pg_has_role('admin', 'spike_owner', 'MEMBER')").await;
    sql(
        &fresh,
        "5",
        "select r.rolname as role, m.rolname as member, g.rolname as grantor, \
         am.admin_option, am.inherit_option, am.set_option from pg_auth_members am \
         join pg_roles r on r.oid = am.roleid join pg_roles m on m.oid = am.member \
         join pg_roles g on g.oid = am.grantor \
         where r.rolname = 'spike_owner'",
    )
    .await;
    sql(&fresh, "5", &format!("select count(*) from {SCHEMA}.users")).await;
    drop(fresh);

    println!("== step 6b: owner-side grant (here the owner can sign in; in Arco it is the console Lambda)");
    let owner = connect(env, OWNER).await?;
    sql(&owner, "6", &format!("GRANT SELECT ON {SCHEMA}.users TO {READER}")).await;

    println!("== step 6c: connect as {READER}");
    let reader = connect(env, READER).await?;
    sql(&reader, "6", "select current_user, current_schema()").await;
    sql(&reader, "6", "select count(*) from users").await;
    sql(&reader, "6", "select count(*) from orders").await;
    sql(&reader, "6", "insert into users values (99, 'x', null)").await;
    sql(&reader, "6", "delete from users where id = 1").await;
    sql(&reader, "6", "create table t (id int primary key)").await;
    sql(&reader, "6", "create table public.t (id int primary key)").await;
    drop(reader);

    println!("== step 6d: remove {READER} (owner REVOKE, then admin)");
    sql(&admin, "6", &format!("DROP ROLE {READER}")).await;
    sql(&owner, "6", &format!("REVOKE SELECT ON {SCHEMA}.users FROM {READER}")).await;
    sql(&owner, "6", &format!("REVOKE SELECT ON {SCHEMA}.orders FROM {READER}")).await;
    sql(&admin, "6", &format!("AWS IAM REVOKE {READER} FROM '{}'", env.iam_role_arn)).await;
    sql(&admin, "6", &format!("REVOKE USAGE ON SCHEMA {SCHEMA} FROM {READER}")).await;
    sql(&admin, "6", &format!("DROP ROLE {READER}")).await;
    Ok(())
}

async fn teardown(env: &Env) -> Result<(), BoxError> {
    println!("== teardown");
    if let Ok(owner) = connect(env, OWNER).await {
        for t in ["users", "orders", r#""Order Items""#] {
            sql(&owner, OWNER, &format!("DROP TABLE IF EXISTS {SCHEMA}.{t}")).await;
        }
    }
    let admin = connect(env, "admin").await?;
    sql(&admin, "admin", &format!("REVOKE {OWNER} FROM admin")).await;
    for role in [READER, OWNER] {
        sql(&admin, "admin", &format!("AWS IAM REVOKE {role} FROM '{}'", env.iam_role_arn)).await;
    }
    sql(&admin, "admin", &format!("REVOKE USAGE ON SCHEMA {SCHEMA} FROM {READER}")).await;
    sql(&admin, "admin", &format!("REVOKE USAGE, CREATE ON SCHEMA {SCHEMA} FROM {OWNER}")).await;
    sql(&admin, "admin", &format!("DROP SCHEMA IF EXISTS {SCHEMA}")).await;
    for role in [READER, OWNER] {
        sql(&admin, "admin", &format!("DROP ROLE IF EXISTS {role}")).await;
    }
    sql(&admin, "admin", "select * from sys.iam_pg_role_mappings").await;
    sql(&admin, "admin", "select rolname from pg_roles where rolname like 'spike%'").await;
    Ok(())
}
