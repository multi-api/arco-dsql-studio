# Arco DSQL Studio MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A macOS desktop client for Amazon Aurora DSQL that signs in with the user's AWS profile (SSO included), finds their clusters, opens in the right schema, browses and edits tables, runs SQL, and explains DSQL's errors and limits.

**Architecture:** A Tauri 2 app. All AWS and Postgres work lives in a plain Rust library crate, `dsql-core`, testable without Tauri; `src-tauri` is a thin layer of Tauri commands over it; the React + TypeScript UI calls those commands and never sees credentials or tokens.

**Tech Stack:** Rust stable (1.99 or later; see CLAUDE.md "Toolchain" for a fresh machine), Tauri 2.x (not the 3.0 alpha), `aws-config` 1.x (`sso` feature), `aws-sdk-dsql`, `aws-sdk-sts`, `aws-sdk-iam`, `aws-sdk-sso`, `aws-sdk-ssooidc`, `aws-sdk-lambda`, `tokio-postgres` 0.7 + `tokio-postgres-rustls` + `rustls` + `webpki-roots`, `rusqlite` (bundled), `sha1`, `serde`, `thiserror`; Node 22+ (26 works), Vite, React 19 (what `create-tauri-app` scaffolds; Testing Library supports it), TypeScript, CodeMirror 6 (`@codemirror/lang-sql`), TanStack Table + TanStack Virtual, Tailwind, Vitest.

**Spec:** `docs/superpowers/specs/2026-11-04-arco-dsql-studio-design.md` (section numbers below refer to it).

## Global Constraints

- DSQL only; database name is always `postgres`; TLS `verify-full` on every connection.
- `admin` uses the admin token (`dsql:DbConnectAdmin`); every other role uses the normal token (`dsql:DbConnect`); token expiry 900 s; a fresh token per new connection.
- Connections are recycled before 55 minutes (DSQL closes them at 60).
- The app stores **no secrets**: no tokens, no keys. SSO tokens only in `~/.aws/sso/cache`, in the AWS CLI's format.
- `~/.aws/config` is only ever appended to, after showing the exact lines and getting confirmation.
- The app never creates IAM users, roles, policies or access keys.
- Production edits are allowed but always previewed and confirmed; read-only connections block writes.
- Public MIT repository: never commit account IDs, cluster IDs, ARNs or profile names. Integration tests read `DSQL_HOST`, `DSQL_REGION`, `AWS_PROFILE` (and `DSQL_TEST_SCHEMA`, `DSQL_CONSOLE_FUNCTION` where needed) and are skipped when unset.
- Before any AWS call (spike, integration tests, manual checks): confirm the profile and account with the owner. Destructive AWS actions are the owner's. SQL experiments clean up after themselves.
- Owner-side SQL on the test app (grants by the tables' owner) is run only with the owner's OK.
- Talk to the owner in Spanish; code, comments, docs and commit messages in English.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Nothing is pushed, no GitHub repo is created, nothing is published without the owner's explicit "si".
- Every user-visible behaviour is documented (README + `docs/`), in the same task that builds it.
- The UI uses the Arco brand in `assets/brand/README.md` (logo, `#C93F1C` tile, light and dark primaries, Instrument Sans); never recolor the logo. App icons are generated from `assets/brand/arco-logo-1024.png` with `npm run tauri icon` (desktop sizes only).

## Review Focus

1. **Identifiers that need quoting** (schema `myapp_staging` is fine, but tables or columns with capitals, spaces, quotes or reserved words like `order`, `user`): every generated statement must quote identifiers and literals correctly. Pinned in Task 10 (`edits_quote_hostile_identifiers`) and Task 11 (`role_plan_quotes_schema`).
2. **Expired SSO session mid-use** (the user leaves the app open overnight): the next connect must fail with "Sign in again", not a raw SDK error, and must not corrupt open tabs. Pinned in Task 3 (`classify_sdk_error_expired_sso`) and Task 9 (`explain_expired_sso`).
3. **A long-lived session crossing the 60-minute limit or a dropped network**: the pool reconnects transparently when no transaction is open, and reports clearly when one was. Pinned in Task 5 (`pool_recycles_old_connections`).
4. **Large saves and large results** (editing 5,000 rows; `select *` on a big table): saves are split under the DSQL row limit, and query results are capped (default 10,000 rows, with a "results truncated" notice). Pinned in Task 10 (`edits_batch_under_row_limit`) and Task 7 (`run_script_caps_rows`).
5. **NULL vs empty string vs the text "NULL"** in the grid: they must round-trip distinctly through display and edit. Pinned in Task 7 (`simple_query_preserves_null`) and Task 10 (`edits_null_vs_empty`).

---

## File Structure

```
arco-dsql-studio/
├── Cargo.toml                    # workspace: crates/dsql-core, src-tauri
├── LICENSE                       # MIT
├── README.md
├── package.json, vite.config.ts, tsconfig.json, index.html, tailwind config
├── docs/
│   ├── superpowers/{specs,plans}/
│   ├── spike-findings.md         # Task 1
│   ├── connecting.md             # profiles, SSO wizard, clusters, roles, IAM permissions
│   ├── working-with-data.md      # table browser, edits, SQL editor, history, warnings
│   ├── team-access.md            # role wizard and the recipe
│   └── development.md            # build, test, integration env vars
├── crates/dsql-core/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   ├── error.rs              # CoreError
│   │   ├── profiles.rs           # ~/.aws/config parsing, appending profiles
│   │   ├── sso.rs                # cache key, cache file, device flow, account/role listing
│   │   ├── identity.rs           # credentials, caller identity, IAM role ARN resolution
│   │   ├── clusters.rs           # region list, ListClusters, endpoints
│   │   ├── token.rs              # auth token generation
│   │   ├── conn.rs               # TLS connect, search_path, pool, recycle, cancel
│   │   ├── catalog.rs            # schemas, tables, columns, indexes, privileges, default schema
│   │   ├── exec.rs               # run_script via simple_query, result model, row cap
│   │   ├── store.rs              # rusqlite: saved connections, history, settings
│   │   ├── classify.rs + rules.rs# statement classification (spec 6.6)
│   │   ├── explain.rs            # error explanations (spec 6.7)
│   │   ├── quote.rs              # identifier/literal quoting
│   │   ├── edits.rs              # grid change set -> SQL, batching
│   │   ├── occ.rs                # retry on 40001
│   │   ├── roles.rs              # role wizard plans (create/remove/check)
│   │   └── console.rs            # invoke the Arco console Lambda
│   ├── tests/fixtures/aws/...    # sample config/credentials/sso cache files
│   ├── tests/integration.rs      # gated by env vars
│   └── examples/spike.rs         # Task 1
├── src-tauri/
│   ├── Cargo.toml, tauri.conf.json, capabilities/default.json, icons/
│   └── src/{main.rs, state.rs, commands/*.rs}
└── src/                          # React UI
    ├── main.tsx, App.tsx, api.ts (typed invoke wrappers), types.ts
    ├── screens/{Welcome,SetupAws,ClusterPicker,Connect,Workspace}.tsx
    ├── components/{SchemaTree,SqlEditor,ResultsGrid,TableBrowser,SaveDialog,HistoryPanel,RoleWizard,ExplainBox,TopBar}.tsx
    └── __tests__/
```

`CoreError` (Task 0) is the single error type of `dsql-core`: `enum CoreError { Aws { kind: AwsErrorKind, message: String }, Db { sqlstate: Option<String>, message: String, detail: Option<String>, hint: Option<String> }, Io(String), Config(String), Cancelled }` with `AwsErrorKind { SsoExpired, AccessDenied, NotAvailableInRegion, Other }`. It is `Serialize` so Tauri commands return it as-is.

---

### Task 0: Repository scaffold

**Files:** Create everything at the top level of the File Structure, `crates/dsql-core/{Cargo.toml,src/lib.rs,src/error.rs}`, `src-tauri/*`, `src/main.tsx`, `src/App.tsx`, `LICENSE`, `README.md` (skeleton), `docs/development.md`, `.gitignore` (target, node_modules, dist, `.env*`).

**Interfaces:**
- Produces: `dsql_core::error::{CoreError, AwsErrorKind}`; `npm run tauri dev` opens a window titled "Arco DSQL Studio"; `cargo test -p dsql-core` runs.

- [x] **Step 1:** Scaffold with `npm create tauri-app@latest` (React + TypeScript, Tauri 2), move into the workspace layout above, set `identifier` to `cl.multiapi.arco-dsql-studio`, product name "Arco DSQL Studio", window 1280×800.
- [x] **Step 2:** Add `dsql-core` with `CoreError` and a test `core_error_serializes_with_kind` asserting `serde_json::to_value(CoreError::Aws{kind: SsoExpired, ..})["Aws"]["kind"] == "SsoExpired"`.
- [x] **Step 3:** Run `cargo test -p dsql-core` → PASS; `npm run tauri dev` → window opens; `npm test` (Vitest) → PASS with one smoke test rendering `<App/>`.
- [x] **Step 4:** `LICENSE` (MIT, Joaquín Gamboa, same as the Laravel driver), `README.md`, `CLAUDE.md` and `.gitignore` already exist; keep them and add the `.gitignore` entries the scaffold needs. Write `docs/development.md` (toolchain, commands).
- [x] **Step 5:** Commit `chore: scaffold Tauri app and dsql-core crate`.

Done 2026-11-04 on Windows 11 (the first machine available; macOS stays the target): the window opens with `npm run tauri dev`, `cargo test --workspace` and `npm test` pass. Root `Cargo.toml` is the workspace, so `target/` is at the root; the Tauri crate is `arco-dsql-studio` (lib `arco_dsql_studio_lib`).

### Task 1: Spike against the real cluster (spec section 8)

Confirm the profile and account with the owner first. Values come from env vars; the owner gives you the real values (they are never in the repository).

**Files:** Create `crates/dsql-core/examples/spike.rs`, `docs/spike-findings.md`.

- [x] **Step 1:** In `spike.rs`, connect with a hand-built token (use `aws_sdk_dsql::auth_token::AuthTokenGenerator`) as `admin` over `tokio-postgres-rustls` with verify-full, run `select current_user, version()`. Record result.
- [x] **Step 2:** In one transaction run, in turn: `client.query("select $1::int", &[&1])` (named prepared statement + drop), `client.query_typed(...)`, `client.simple_query(...)`, then `select 1`. Record which (if any) produce `25P02` or a protocol error. **Decision:** `exec` uses `simple_query`; edits use `query_typed` if it is safe, otherwise literal SQL built by `quote.rs`.
- [x] **Step 3:** Start `select pg_sleep(30)`, call `CancelToken::cancel_query` after 2 s. Record whether it returns `57014` or the connection must be dropped.
- [x] **Step 4:** Time the catalog queries Task 6 needs (copy them from Task 6) against the test schema. Record times and any unsupported view/function.
- [x] **Step 5:** As admin: `GRANT "<app_role>" TO admin; SET ROLE "<app_role>"; select count(*) from <a table>; RESET ROLE; REVOKE "<app_role>" FROM admin;`. Record each statement's outcome. Leave nothing in place.
- [x] **Step 6:** Connect as a custom role created and removed with the spec 6.5 recipe (owner-side grants via the console Lambda, owner's OK first). Confirm search_path, SELECT, and the permission-denied codes.
- [x] **Step 7:** Check the current DSQL quotas (rows per transaction, transaction duration, `CREATE INDEX` without `ASYNC`) in the AWS docs; record them with links.
- [x] **Step 8:** Write `docs/spike-findings.md` (no account-specific values), update Task 5/7/10/11 notes in this plan if a decision changed, commit `docs: DSQL spike findings`.

Done 2026-11-04, see `docs/spike-findings.md`. The owner chose a scratch cluster instead of the Arco test app (a real app), so `spike.rs setup` builds a stand-in owner role and schema, and Step 6's owner-side grant ran as that role instead of through the console Lambda. Decisions that changed: cancel drops the connection (DSQL ignores cancel requests); primary keys come from the first `indnkeyatts` entries of `pg_index.indkey`; `CREATE INDEX` without `ASYNC` is always rejected; `SET ROLE` is unsupported, but the admin takeover works through role membership alone.

### Task 2: AWS profiles

**Files:** Create `crates/dsql-core/src/profiles.rs`, fixtures `tests/fixtures/aws/config_mixed`, `credentials_mixed`.

**Interfaces:**
- Produces:
  - `pub struct ProfileInfo { name: String, kind: ProfileKind, sso_session: Option<String>, sso_start_url: Option<String>, sso_region: Option<String>, sso_account_id: Option<String>, sso_role_name: Option<String>, region: Option<String> }`
  - `pub enum ProfileKind { Sso, AccessKeys, AssumeRole, Process, Other }`
  - `pub struct AwsPaths { config: PathBuf, credentials: PathBuf, sso_cache: PathBuf }` with `AwsPaths::from_env() -> AwsPaths` (honours `AWS_CONFIG_FILE`, `AWS_SHARED_CREDENTIALS_FILE`; `sso_cache` = `~/.aws/sso/cache`)
  - `pub fn list_profiles(paths: &AwsPaths) -> Result<Vec<ProfileInfo>, CoreError>`
  - `pub fn render_sso_profile(p: &NewSsoProfile) -> String` and `pub fn append_profile(paths: &AwsPaths, block: &str) -> Result<(), CoreError>`
  - `pub struct NewSsoProfile { profile: String, session: String, start_url: String, sso_region: String, account_id: String, role_name: String, region: String }`

- [x] **Step 1: Failing tests**
  - `lists_sso_session_profile` (profile with `sso_session = corp` + `[sso-session corp]` → kind Sso, start url resolved from the session).
  - `lists_legacy_sso_profile`.
  - `lists_access_key_profile_from_credentials_only` (a profile present only in `credentials` → AccessKeys).
  - `default_profile_named_default`.
  - `render_sso_profile_matches_cli_format` (exact text: `[profile x]\nsso_session = s\nsso_account_id = ...\nsso_role_name = ...\nregion = ...\n\n[sso-session s]\nsso_start_url = ...\nsso_region = ...\nsso_registration_scopes = sso:account:access\n`).
  - `append_never_rewrites_existing_lines` (file bytes before are a prefix of bytes after).
  - `append_refuses_existing_profile_name` → `CoreError::Config`.
- [x] **Step 2:** Run `cargo test -p dsql-core profiles` → FAIL.
- [x] **Step 3:** Implement with a small INI reader (sections `[profile x]`, `[default]`, `[sso-session x]`; `#`/`;` comments; ignore unknown keys).
- [x] **Step 4:** Run → PASS. Commit `feat(core): list and append AWS profiles`.

Done 2026-11-04. Extra tests beyond the list: assume-role and process kinds, nested settings ignored, profile order, missing files, a missing config created on append, duplicate `[sso-session]` refused, `AWS_CONFIG_FILE`/`AWS_SHARED_CREDENTIALS_FILE` overrides. Checked by hand against a real SSO profile on Windows.

### Task 3: SSO sign-in, credentials and identity

**Files:** Create `src/sso.rs`, `src/identity.rs`, fixture `tests/fixtures/aws/sso_cache_sample.json`.

**Interfaces:**
- Consumes: `ProfileInfo`, `AwsPaths`.
- Produces:
  - `pub fn sso_cache_key(session_name: Option<&str>, start_url: &str) -> String` (lowercase hex SHA-1 of the session name when present, else of the start URL).
  - `pub fn write_sso_cache(paths: &AwsPaths, key: &str, token: &SsoToken) -> Result<(), CoreError>`.
  - `pub struct SsoToken { access_token, expires_at: DateTime<Utc>, refresh_token: Option<String>, client_id, client_secret, registration_expires_at, region, start_url }`, serialised with the CLI's camelCase keys (`accessToken`, `expiresAt` as `YYYY-MM-DDTHH:MM:SSZ`, `refreshToken`, `clientId`, `clientSecret`, `registrationExpiresAt`, `region`, `startUrl`).
  - `pub async fn start_device_login(start_url: &str, sso_region: &str) -> Result<DeviceLogin, CoreError>` → `DeviceLogin { verification_uri_complete, user_code, device_code, interval, expires_in, client_id, client_secret, client_secret_expires_at }`.
  - `pub async fn finish_device_login(login: &DeviceLogin, start_url: &str, sso_region: &str) -> Result<SsoToken, CoreError>` (polls `CreateToken`, honours `slow_down` / `authorization_pending`).
  - `pub async fn list_accounts_and_roles(token: &SsoToken) -> Result<Vec<SsoAccountRole>, CoreError>` → `SsoAccountRole { account_id, account_name, role_name }`.
  - `pub async fn sdk_config(profile: &str, region: &str) -> Result<aws_config::SdkConfig, CoreError>`.
  - `pub async fn caller_identity(cfg: &SdkConfig) -> Result<CallerIdentity, CoreError>` → `{ account, arn, user_id }`.
  - `pub fn classify_sdk_error(msg: &str) -> AwsErrorKind`.
  - `pub fn assumed_role_name(sts_arn: &str) -> Option<String>`.
  - `pub fn sso_role_arn_guess(account: &str, role_name: &str, sso_region: &str) -> String` (`arn:aws:iam::<a>:role/aws-reserved/sso.amazonaws.com/<region>/<role>`; for SSO region `us-east-1` there is no region segment, confirmed in Task 1 with `iam:GetRole`; pin with a test).
  - `pub async fn iam_role_arn(cfg: &SdkConfig, role_name: &str) -> Result<String, CoreError>` (IAM `GetRole`).

- [x] **Step 1: Failing tests:** `sso_cache_key_session` (`sso_cache_key(Some("my-sso"), "https://x.awsapps.com/start")` == SHA-1 hex of `my-sso`), `sso_cache_key_legacy`, `sso_cache_roundtrip_matches_cli_keys` (serialise → keys equal the fixture's), `classify_sdk_error_expired_sso` (messages containing "The SSO session associated with this profile has expired" or "Token has expired" → `SsoExpired`), `classify_access_denied`, `assumed_role_name_parses_sso_arn` (`arn:aws:sts::123:assumed-role/AWSReservedSSO_Admin_abc/me@x` → `AWSReservedSSO_Admin_abc`), `sso_role_arn_guess_*`.
- [x] **Step 2:** Run → FAIL. **Step 3:** Implement. **Step 4:** Run → PASS.
- [x] **Step 5:** Integration test `it_caller_identity` (gated) → prints the account; owner confirms it is the test account.
- [x] **Step 6:** Commit `feat(core): SSO device login, CLI-compatible cache, caller identity`.

Done 2026-11-04. `SsoToken`'s client and refresh fields are `Option` (legacy CLI tokens lack them); `read_sso_cache` added; `sdk_config` resolves credentials once so an expired session fails early with `SsoExpired`; `UnauthorizedException: Session token not found or invalid` counts as `SsoExpired`. Checked live: `it_caller_identity` (GetRole allowed) and `examples/sso_login.rs` (device flow, 11 account/role pairs, token file identical in name, keys and formats to the CLI's, written to a temp dir).

### Task 4: Cluster discovery

**Files:** Create `src/clusters.rs`.

**Interfaces:**
- Produces:
  - `pub const DSQL_REGIONS: &[&str]` (the regions where DSQL is available at build time, from the AWS docs; keep in one place).
  - `pub struct ClusterInfo { identifier, region, status, endpoint, arn, tags: BTreeMap<String,String> }`.
  - `pub fn endpoint(identifier: &str, region: &str) -> String` (`<id>.dsql.<region>.on.aws`).
  - `pub async fn list_clusters(base: &SdkConfig, regions: &[&str]) -> (Vec<ClusterInfo>, Vec<(String, CoreError)>)` (parallel; access denied / not-available regions go into the second vector, never fail the whole call).
  - `pub fn parse_endpoint(host: &str) -> Option<(String, String)>` (identifier, region) for manual entry.

- [x] **Step 1: Failing tests:** `endpoint_format`, `parse_endpoint_roundtrip`, `parse_endpoint_rejects_non_dsql_host`.
- [x] **Step 2–4:** FAIL → implement (ListClusters paginated, then GetCluster per cluster for status/endpoint, ListTagsForResource for tags) → PASS.
- [x] **Step 5:** Gated integration test `it_lists_test_cluster` (asserts the `DSQL_HOST` cluster is in the list).
- [x] **Step 6:** Commit `feat(core): discover DSQL clusters across regions`.

Done 2026-11-05. `DSQL_REGIONS` (20) comes from AWS's public SSM parameters, not the docs. `GetCluster` already returns the tags, so `ListTagsForResource` is not called (Task 17's IAM table: check whether `dsql:GetCluster` alone shows tags for a least-privilege role). Live: 2 clusters across 20 regions in ~3 s; the 4 opt-in regions not enabled on the account failed with `UnrecognizedClientException` and were skipped as `NotAvailableInRegion`.

### Task 5: Tokens, TLS connections and the pool

**Files:** Create `src/token.rs`, `src/conn.rs`.

**Interfaces:**
- Consumes: `sdk_config`.
- Produces:
  - `pub enum DbRole { Admin, Custom(String) }` with `DbRole::user(&self) -> &str`.
  - `pub async fn auth_token(cfg: &SdkConfig, host: &str, region: &str, role: &DbRole) -> Result<String, CoreError>` (admin variant for `Admin`, 900 s).
  - `pub struct Target { profile: String, region: String, host: String, role: DbRole, schema: Option<String> }`.
  - `pub struct Session` wrapping a `tokio_postgres::Client`, its `CancelToken`, `opened_at: Instant`.
  - `pub async fn connect(target: &Target) -> Result<Session, CoreError>` (TLS verify-full with `webpki-roots`, `dbname=postgres`, then `SET search_path = <quoted schema>` when set).
  - `pub struct Pool` with `Pool::new(target: Target, max: usize)`, `async fn get(&self) -> Result<PooledSession, CoreError>`, recycling sessions older than `RECYCLE_AFTER = 55 min`, and discarding sessions whose last error was a closed connection.
  - `pub fn is_too_old(opened: Instant, now: Instant) -> bool`.
  - `Session::cancel(&self)`: DSQL ignores cancel requests (Task 1, Step 3), so send `cancel_query`, then drop the connection; the pool replaces it. The UI says the statement may still finish on the server.
- [x] **Step 1: Failing tests:** `pool_recycles_old_connections` (`is_too_old` true at 55 min, false at 54), `search_path_statement_quotes` (`set_search_path_sql("My Schema")` == `SET search_path = "My Schema"`), `db_role_user`.
- [x] **Step 2–4:** FAIL → implement → PASS.
- [x] **Step 5:** Gated `it_connect_admin_sets_search_path` (`select current_schema()` == `DSQL_TEST_SCHEMA`).
- [x] **Step 6:** Commit `feat(core): IAM tokens, verify-full TLS and a recycling pool`.

Done 2026-11-05. Also `connect_with(cfg, target)` (the pool loads credentials once), `quote::ident` (Task 10 adds `literal`), `PooledSession::discard()` for sessions that may hold an open transaction (Task 7/10 call it on errors mid-transaction), and `Pool::clear()`. `Session::cancel` sends a cancel request (2 s timeout) and then aborts the connection task: live, `pg_sleep(30)` stopped after ~1.4 s with `connection closed`, and the pool opened a fresh session next. Extra gated tests: `it_cancel_closes_the_connection`, `it_pool_reuses_idle_sessions`. The integration fixture is `spike.rs setup` on the scratch cluster (`DSQL_TEST_SCHEMA=spike_app`).

### Task 6: Catalog

**Files:** Create `src/catalog.rs`.

**Interfaces:**
- Consumes: `Session`.
- Produces:
  - `pub struct SchemaInfo { name, owner }`, `TableInfo { schema, name, kind: TableKind /*Table,View,Sequence*/, owner, can_select, can_insert, can_update, can_delete }`, `ColumnInfo { name, data_type, nullable, default, is_pk, ordinal }`, `IndexInfo { name, columns, unique }`.
  - `pub async fn schemas(s: &Session) -> Result<Vec<SchemaInfo>, CoreError>` (excludes `pg_catalog`, `information_schema`, `sys`, `pg_toast`, `pg_temp_*`, and `public` when empty).
  - `pub async fn tables(s, schema) -> Result<Vec<TableInfo>>` (`pg_class`/`pg_namespace` + `has_table_privilege`; not `information_schema.table_privileges`, which returns nothing for `admin`).
  - `pub async fn columns(s, schema, table)`, `pub async fn indexes(s, schema, table)`, `pub async fn primary_key(s, schema, table) -> Result<Vec<String>>` (only the first `indnkeyatts` entries of `pg_index.indkey`: DSQL primary-key indexes cover every column, Task 1 Step 4; test `primary_key_uses_key_columns_only` on the SQL).
  - `pub fn pick_default_schema(schemas: &[SchemaInfo], role: &DbRole, current_user: &str) -> Option<String>` — rules in order: the role's own default search_path schema if it exists; the only non-public schema; a schema equal to `current_user`; else the schema matching `^[a-z0-9_]+_(production|staging|[a-z0-9]+)$` with the most tables is **not** guessed — return `None` and let the UI ask.
  - `pub fn looks_like_production(schema: &str, tags: &BTreeMap<String,String>) -> bool` (suffix `_production`/`_prod`, or a tag value `production`).
- [x] **Step 1: Failing tests:** `pick_default_single_schema`, `pick_default_schema_named_like_user`, `pick_default_none_when_ambiguous`, `looks_like_production_suffix`, `looks_like_production_tag`.
- [x] **Step 2–4:** FAIL → implement → PASS. Gated `it_lists_test_schema_tables` (≥1 table, all owned by the app role, `can_select == false` as admin unless granted).
- [x] **Step 5:** Commit `feat(core): catalog queries and default schema`.

Done 2026-11-05. `pick_default_schema` takes `role_search_path: Option<&str>` instead of `&DbRole` (the role's own `ALTER ROLE ... SET search_path`, read by the new `role_search_path(s)` from `pg_roles.rolconfig` and parsed by `first_schema_in_role_config`, quotes included). `indexes` lists key columns only. Live on `spike_app` as admin: tables listed with `can_select = false`, primary keys `["id"]`, the table without a primary key has none. Reading the whole catalog for 3 tables took ~4 s (each query is a ~150 ms round trip), so the tree in Task 14 must load columns and indexes only when a table is expanded.

### Task 7: Running SQL, results, history

**Files:** Create `src/exec.rs`, `src/store.rs`.

**Interfaces:**
- Consumes: `Session`, `classify` is **not** called here (the UI calls it before running).
- Produces:
  - `pub struct ResultSet { columns: Vec<String>, rows: Vec<Vec<Option<String>>>, truncated: bool }`.
  - `pub enum StatementOutcome { Rows(ResultSet), Command { tag: String, rows_affected: u64 }, Error(CoreError) }`.
  - `pub struct ScriptResult { outcomes: Vec<StatementOutcome>, duration_ms: u64 }`.
  - `pub const MAX_ROWS: usize = 10_000`.
  - `pub async fn run_script(s: &Session, sql: &str, max_rows: usize) -> ScriptResult` (`simple_query`; stops at the first error; NULL stays `None`).
  - `pub struct Store` (`rusqlite`, file `studio.sqlite` in the Tauri app data dir) with `open(path)`, `save_connection(&SavedConnection) -> i64`, `list_connections()`, `delete_connection(id)`, `add_history(&HistoryEntry)`, `search_history(q: &str, limit: usize)`, `clear_history()`, `get_setting/set_setting`.
  - `pub struct SavedConnection { id: Option<i64>, name, profile, region, cluster_id, host, role: String /*"admin" or name*/, schema: Option<String>, production: bool, read_only: bool }`; `HistoryEntry { connection_id, sql, started_at, duration_ms, rows: Option<u64>, error: Option<String> }`.
  - Schema migrations by `PRAGMA user_version`.
- [x] **Step 1: Failing tests:** `simple_query_preserves_null` (unit test on the row-conversion function: `SimpleQueryRow` value `None` → `None`, `""` → `Some("")`, `"NULL"` → `Some("NULL")`), `run_script_caps_rows` (conversion of 10,001 rows with `max_rows=10_000` → 10,000 rows, `truncated == true`), `store_roundtrip_connection`, `store_history_search`, `store_holds_no_secret_columns` (schema has no column named like `token|secret|password|key`).
- [x] **Step 2–4:** FAIL → implement → PASS. Gated `it_run_script_multi_statement`.
- [x] **Step 5:** Commit `feat(core): run SQL scripts and keep local history`.

Done 2026-11-05. New `split.rs` (`split_statements`: `;` outside quotes, comments, nested block comments, `E''` escapes and dollar quotes): `run_script` runs each statement on its own, because Postgres wraps a multi-statement simple query in one implicit transaction and DSQL allows one DDL per transaction. Task 8's keyword fallback reuses `split_statements`. `run_script` streams with `simple_query_raw` and stops reading at the cap. `Command.tag` comes from the statement's leading keywords (`command_tag`), since tokio-postgres only returns the row count. `HistoryEntry` gained `id`; its `connection_id` is optional; the settings table uses `name`/`value` columns. Live: NULL, `''` and `'NULL'` stay distinct, a 25-row result capped at 10 is marked truncated, the script stops at `42501`, and a script with two `CREATE TABLE`s runs.

### Task 8: Statement classification (spec 6.6)

**Files:** Create `src/classify.rs`, `src/rules.rs`.

**Interfaces:**
- Produces:
  - `pub enum StatementKind { Read, Write, Schema, Transaction, Other }`.
  - `pub struct Finding { rule_id: &'static str, message: String, alternative: String, docs_url: Option<String> }`.
  - `pub struct Classified { statements: Vec<ClassifiedStatement> }`, `ClassifiedStatement { sql: String, kind: StatementKind, findings: Vec<Finding> }`.
  - `pub fn classify(sql: &str) -> Classified` (statements from `split::split_statements`, then a small tokenizer and keyword patterns; see the Task 8 note).
  - `pub fn needs_confirmation(c: &Classified, production: bool) -> bool` and `pub fn blocked_by_read_only(c: &Classified) -> bool`.
  - Rule ids (one test each, named `rule_<id>`): `truncate`, `add_constraint`, `add_column_default_or_not_null`, `alter_column_type`, `set_not_null`, `savepoint`, `temp_table`, `trigger`, `plpgsql`, `extension`, `advisory_lock`, `for_share`, `fts_config`, `create_database`, `index_without_async` (every `CREATE INDEX` without `ASYNC`, empty table or not: DSQL answers `0A000 unsupported mode. please use CREATE INDEX ASYNC.`), `multiple_ddl_in_transaction`. Messages and alternatives from spec 6.6.
- [x] **Step 1: Failing tests:** one per rule id above, plus `select_is_read`, `insert_is_write`, `grant_is_schema`, `comment_and_string_with_semicolon_not_split` (`select ';' -- ;` is one statement), `production_write_needs_confirmation`, `read_only_blocks_delete`.
- [x] **Step 2–4:** FAIL → implement → PASS. Commit `feat(core): classify statements DSQL rejects`.

Done 2026-11-05. Classification uses its own tokenizer instead of `sqlparser`: DSQL syntax (`CREATE INDEX ASYNC`, `AWS IAM GRANT`) is unknown to general parsers, and the rules only need keywords outside strings and comments; the spec was updated. `multiple_ddl_in_transaction` also fires for DDL next to data changes inside `BEGIN ... COMMIT` (AWS: DDL and DML go in separate transactions). Rule texts and docs links are in `rules.rs`. Extra tests: strings, comments and quoted identifiers never trigger rules; `WITH ... DELETE` and `EXPLAIN ANALYZE DELETE` are writes; `DO` blocks are PL/pgSQL; no false positives for `CREATE INDEX ASYNC`, nullable `ADD COLUMN`, `FOR UPDATE`, `to_tsvector('simple', ...)`.

### Task 9: Error explanations (spec 6.7)

**Files:** Create `src/explain.rs`.

**Interfaces:**
- Consumes: `CoreError`, `catalog::TableInfo`.
- Produces:
  - `pub struct ExplainContext { role: String, search_path: Option<String>, caller_arn: Option<String>, is_admin: bool, known_tables: Vec<TableInfo> /*all schemas*/ }`.
  - `pub enum FixAction { SwitchSchema(String), ShowGrant { table: String, owner: String, sql: String }, SignIn, Retry, Rollback, None }`.
  - `pub struct Explanation { title: String, body: String, fix: FixAction }`.
  - `pub fn explain(err: &CoreError, ctx: &ExplainContext) -> Explanation`.
- [x] **Step 1: Failing tests:** `explain_relation_in_other_schema` (`relation "users" does not exist`, users known in schema `app_staging` → `SwitchSchema("app_staging")`), `explain_permission_denied_table` (→ `ShowGrant` with `GRANT SELECT ON "users" TO "readonly"` and owner), `explain_permission_denied_schema`, `explain_auth_failed_admin` (mentions `dsql:DbConnectAdmin` and the caller ARN), `explain_auth_failed_custom` (mentions `dsql:DbConnect` and `AWS IAM GRANT`), `explain_expired_sso` (→ `SignIn`), `explain_occ` (40001 → `Retry`), `explain_aborted_tx` (25P02 → `Rollback`), `explain_unknown_keeps_raw` (body contains SQLSTATE and message).
- [x] **Step 2–4:** FAIL → implement → PASS. Commit `feat(core): explain DSQL errors in plain language`.

Done 2026-11-05. Grant snippets are schema-qualified (`GRANT SELECT ON "app_staging"."users" TO "readonly";`) and quote hostile names; `permission denied for schema` gives a `GRANT USAGE` snippet. Probed live: DSQL answers an unmapped or missing role with `28000 unable to accept connection, access denied` (same text as a missing IAM permission), and a token signed for the wrong region with `08006` plus the hint `Credential should be scoped to a valid region`; both are explained. Also explained: AWS access denied, region unavailable, `0A000`. Gated `it_explains_real_errors` runs real 42P01, 42501 and 28000 errors through `explain`.

### Task 10: Grid edits, quoting, OCC retry

**Files:** Create `src/quote.rs`, `src/edits.rs`, `src/occ.rs`.

**Interfaces:**
- Produces:
  - `pub fn ident(s: &str) -> String` (always double-quoted, `"` doubled), `pub fn literal(v: Option<&str>) -> String` (`NULL` or `'…'` with `'` doubled; reject NUL bytes with `CoreError::Config`).
  - `pub struct ChangeSet { schema, table, pk: Vec<String>, updates: Vec<RowUpdate>, inserts: Vec<BTreeMap<String, Option<String>>>, deletes: Vec<BTreeMap<String, Option<String>> /*pk values*/> }`, `RowUpdate { pk_values: BTreeMap<String, Option<String>>, set: BTreeMap<String, Option<String>> }`.
  - `pub fn build_statements(cs: &ChangeSet) -> Result<Vec<String>, CoreError>` (error when `pk` is empty; values sent as text literals with an explicit cast to the column type `'…'::<type>` — column types passed in `ChangeSet.column_types: BTreeMap<String,String>`).
  - `pub const MAX_ROWS_PER_TX: usize = 3000` (Task 1 Step 7; DSQL also caps a write transaction at 10 MiB and 5 minutes) and `pub fn batches(stmts: Vec<String>, max: usize) -> Vec<Vec<String>>`.
  - `pub async fn apply(pool: &Pool, batches: Vec<Vec<String>>, max_retries: u32 /*3*/) -> Result<ApplyReport, CoreError>` (each batch: BEGIN … COMMIT, retry the whole batch on 40001 with jittered backoff 50–400 ms; `ApplyReport { batches_applied, rows_affected, retries }`).
  - `pub fn is_occ(err: &CoreError) -> bool`.
  - (Task 1 showed `query_typed` is safe on DSQL, so the builder may switch `build_statements` to parameters; the tests stay the same at the API level.)
- [x] **Step 1: Failing tests:** `edits_quote_hostile_identifiers` (column `we"ird`, table `Order` → `"we""ird"`, `"Order"`), `edits_null_vs_empty` (`None` → `NULL`, `Some("")` → `''`, `Some("NULL")` → `'NULL'`), `edits_literal_doubles_quotes`, `edits_require_pk`, `edits_update_targets_pk` (`UPDATE "s"."t" SET "name" = 'x'::text WHERE "id" = '1'::bigint`), `edits_batch_under_row_limit` (5,000 updates → 2 batches with `MAX_ROWS_PER_TX = 3000`), `occ_detects_40001`.
- [x] **Step 2–4:** FAIL → implement → PASS. Gated `it_apply_update_roundtrip` on a scratch table the test creates and drops (needs a role that can create tables — admin in `public` or a test schema; skip if not permitted).
- [x] **Step 5:** Commit `feat(core): grid change sets, batching and OCC retry`.

Done 2026-11-05. `quote.rs` already existed (Task 4); Task 10 added `literal`. `apply` returns an `ApplyReport` instead of `Result`: batches commit one by one, so a failure after some batches must still report what was saved; the report has `batches_applied`, `batches_total`, `rows_affected`, `rows_missing` (UPDATE or DELETE that matched no row), `retries` and `error`. Statements run deletes, then updates, then inserts, so a row can be deleted and re-added with the same key in one save. `batches` also closes a batch at about 8 MiB of SQL (`MAX_BYTES_PER_TX`, under DSQL's 10 MiB). Backoff is 50 ms up to 100, 200, then 400 ms, jittered without a `rand` dependency. A failed batch is rolled back and its session reused, or discarded if the rollback fails. Gated `it_apply_update_roundtrip` inserts, updates (NULL vs empty), deletes and misses a row on a scratch table in `public`, then provokes a real lost conflict: DSQL answers `40001 change conflicts with another transaction (OC000)` at COMMIT.

### Task 11: Role wizard and the Arco console

**Files:** Create `src/roles.rs`, `src/console.rs`.

**Interfaces:**
- Consumes: `quote::ident/literal`, `catalog::tables`, `identity::*`.
- Produces:
  - `pub enum Access { ReadOnly, ReadWrite }`.
  - `pub struct RolePlan { admin_sql: Vec<String>, owner_sql: Vec<String>, owner_tinker: String }`.
  - `pub fn create_plan(role: &str, iam_role_arn: &str, schema: &str, tables: &[String], access: Access) -> RolePlan` — admin: `CREATE ROLE "r" WITH LOGIN`, `AWS IAM GRANT "r" TO '<arn>'`, `GRANT USAGE ON SCHEMA "s" TO "r"`, `ALTER ROLE "r" SET search_path = "s"`; owner: `GRANT SELECT[, INSERT, UPDATE, DELETE] ON "s"."t" TO "r"` per table; `owner_tinker` = the PHP one-liner from spec 6.5 adapted to the privileges.
  - `pub fn remove_plan(role, iam_role_arn, schema, tables) -> RolePlan` — owner REVOKEs first, then admin `AWS IAM REVOKE`, `REVOKE USAGE`, `DROP ROLE` (order pinned by test).
  - `pub fn grant_owner_privileges_via_admin_plan(app_role: &str, sql: &[String]) -> Vec<String>` — only used when the user accepts: `GRANT "<app_role>" TO admin`, …sql…, `REVOKE "<app_role>" FROM admin`. Task 1 Step 5: `SET ROLE` is unsupported on DSQL, but membership alone inherits the owner's privileges, including `GRANT`. It needs `admin` to hold ADMIN OPTION on the app role (true when `admin` created it); add `pub async fn admin_can_take_over(s, app_role) -> Result<bool>` (`pg_auth_members.admin_option`) and offer the path only when it is true. Test `admin_takeover_plan_has_no_set_role`.
  - `pub async fn existing_mappings(s: &Session) -> Result<Vec<(String, String)>>` (`sys.iam_pg_role_mappings`).
  - `pub fn console_function_name(schema: &str) -> String` (`myapp_staging` → `myapp-staging-console`: `_` → `-`, suffix `-console`; the UI lets the user correct it).
  - `pub async fn run_tinker(cfg: &SdkConfig, function: &str, php: &str) -> Result<String, CoreError>` (Lambda Invoke, qualifier `live`, payload `{"command":"tinker --execute=\"<escaped php>\""}`; return the output; map FunctionError to `CoreError::Aws{Other}` with the output).
- [x] **Step 1: Failing tests:** `role_plan_quotes_schema`, `create_plan_readonly_grants_select_only`, `create_plan_readwrite`, `remove_plan_revokes_before_drop` (owner_sql all REVOKE; admin_sql ends with `DROP ROLE`; `AWS IAM REVOKE` before it), `owner_tinker_escapes_quotes`, `console_function_name_from_schema`, `tinker_payload_escaping` (php with `"` and `\` → valid JSON whose `command` decodes back).
- [x] **Step 2–4:** FAIL → implement → PASS.
- [x] **Step 5:** Gated `it_role_lifecycle` (owner's OK required; needs `DSQL_CONSOLE_FUNCTION`): create readonly role → connect as it → SELECT works, INSERT denied → remove → `DROP ROLE` succeeded, no mapping left.
- [x] **Step 6:** Commit `feat(core): role wizard plans and Arco console invocation`.

Done 2026-11-05. `create_plan` and `remove_plan` return `Result`: they refuse an empty, reserved (`admin`, `public`, `pg_*`) or over-63-byte role name and an ARN that is not an IAM role ARN (an STS assumed-role ARN is the usual mistake). `remove_plan`'s owner side is `REVOKE ALL ON ...`, so it works whatever access was granted. The tinker snippet is one `DB::statement('...');` per statement; the console payload escapes `\`, `"`, `$` and backticks, since the command may go through a shell. `console_output` reads `{"output": <base64 or text>, "statusCode": n}`, a JSON string, or raw text; a `FunctionError` or non-zero status is an error with the output. Probed live: `AWS IAM GRANT` accepts a quoted role name; `sys.iam_pg_role_mappings` has `pg_role_name` and `arn`. `it_role_lifecycle` ran on the scratch cluster through the admin takeover (no console Lambda there): create read-only role, connect as it, SELECT works, INSERT fails with 42501, remove, no role or mapping left. `it_console_tinker` (a harmless `echo`) is gated on `DSQL_CONSOLE_FUNCTION` and has not run yet: it needs a real Arco app and the owner's OK.

### Task 12: Tauri commands

**Files:** Create `src-tauri/src/state.rs`, `src-tauri/src/commands/{aws.rs,db.rs,store.rs,roles.rs}.rs`; modify `src-tauri/src/main.rs`; `src/api.ts`, `src/types.ts`.

**Interfaces:**
- Consumes: every `dsql-core` API above.
- Produces (Tauri command names; `src/api.ts` exports typed wrappers with the same camelCase names; `types.ts` mirrors the serde structs):
  - AWS: `list_profiles`, `sso_start_login(start_url, sso_region)`, `sso_finish_login(login_id)`, `sso_list_accounts(login_id)`, `preview_sso_profile(new_profile) -> String`, `append_sso_profile(new_profile)`, `caller_identity(profile)`, `list_clusters(profile, regions: Option<Vec<String>>)`.
  - DB: `open_connection(saved: SavedConnection) -> ConnectionHandle { id: String, current_user, search_path }`, `close_connection(id)`, `list_schemas(id)`, `set_schema(id, schema)`, `list_tables(id, schema)`, `table_columns(id, schema, table)`, `table_page(id, schema, table, page: PageRequest { offset, limit, order_by: Vec<(String, bool)>, filters: Vec<Filter>, where_sql: Option<String> }) -> ResultSet + sql: String`, `classify_sql(sql)`, `run_sql(id, sql) -> ScriptResult` (records history), `cancel(id)`, `preview_changes(changeset) -> Vec<Vec<String>>`, `apply_changes(id, changeset) -> ApplyReport`, `explain_error(id, err) -> Explanation`.
  - Store: `list_connections`, `save_connection`, `delete_connection`, `search_history`, `clear_history`, `get_setting`, `set_setting`.
  - Roles: `role_create_plan`, `role_remove_plan`, `role_run_admin(id, sql)`, `role_run_owner_via_console(profile, function, php)`, `role_mappings(id)`.
  - Events: `sso://pending` (user code + URL), `db://occ-retry` (`{ attempt }`).
- Pending SSO logins and open pools live in `AppState` (`Mutex<HashMap<String, …>>`); pools are dropped on `close_connection` and app exit. No token or credential is ever returned to the frontend (test `commands_never_return_tokens`: serialise every command's response type in a unit test and assert no field named `token|access_token|client_secret`).
- `table_page` builds SQL with `quote.rs`; filters use `Filter { column, op: Eq|Ne|Lt|Gt|Contains|IsNull|IsNotNull, value: Option<String> }`; `where_sql` is passed through `classify` and refused unless the statement is `Read`.
- [ ] **Step 1:** Failing tests: `commands_never_return_tokens`, `table_page_sql_builds_filters` (Contains → `"c"::text ILIKE '%v%'` with `%`/`_` escaped), `table_page_rejects_where_with_write`.
- [ ] **Step 2–4:** FAIL → implement → PASS (`cargo test -p arco-dsql-studio`).
- [ ] **Step 5:** Commit `feat(app): Tauri commands over dsql-core`.

### Task 13: Welcome, AWS setup and cluster picker (UI)

**Files:** Create `src/screens/{Welcome,SetupAws,ClusterPicker,Connect}.tsx`, `src/components/ExplainBox.tsx`, tests in `src/__tests__/`.

- Behaviour (spec 5.1–5.3): profile list with kind badges and "Sign in" for SSO profiles; "Set up AWS access" wizard (start URL + region → device code shown with "Open browser" → account/role list → profile name → preview of exact lines → confirm → appended); "Signed in as … in account …"; clusters grouped by region with status and tags (last-used regions remembered with `set_setting("last_regions")` so the next launch only scans those), "Scan all regions", manual endpoint entry; connect-as choice (admin / custom role name) with one-line explanations; schema picker when `pick_default_schema` returns `None`; production flag (prefilled by `looks_like_production`) and read-only toggle; save connection. Errors render through `ExplainBox` (title, body, fix button).
- [ ] **Step 1:** Vitest + Testing Library with `api.ts` mocked: `setup_wizard_shows_exact_lines_before_append` (append not called until confirm), `cluster_picker_skips_denied_regions_quietly`, `connect_asks_schema_when_ambiguous`, `explain_box_switch_schema_calls_set_schema`.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Manual check with the owner's SSO test profile (owner confirms profile/account first): sign in, see the test cluster, connect as admin, land in the test schema. Screenshot in the PR description, not in the repo.
- [ ] **Step 6:** Write `docs/connecting.md` (profiles, SSO wizard, what is written to `~/.aws/config`, clusters, admin vs custom role, the IAM permissions each needs). Commit `feat(ui): sign in, pick a cluster, connect`.

### Task 14: Workspace — schema tree, SQL editor, results, history (UI) — **MVP milestone**

**Files:** Create `src/screens/Workspace.tsx`, `src/components/{TopBar,SchemaTree,SqlEditor,ResultsGrid,HistoryPanel}.tsx`.

- Behaviour (spec 5.4, 5.6): top bar with connection, role, schema, red PRODUCTION badge, read-only indicator; tree schemas → tables/views/sequences → columns/indexes with lock icon when `can_select` is false and privileges on hover; tabs; CodeMirror editor with SQL highlighting and autocomplete from the current schema's tables/columns; Run statement at cursor (⌘↵), Run all (⇧⌘↵), Cancel; classification before run (warning dialog with alternative for findings; confirmation in production for Write/Schema; blocked in read-only); virtualized read-only results grid with row count, duration, truncated notice; errors through `ExplainBox` (an OCC conflict on user SQL offers a one-click "Retry", never an automatic replay); history panel with search, click to open in a new tab, clear button; history off switch in settings.
- [ ] **Step 1:** Failing tests: `run_warns_on_truncate_and_can_proceed`, `production_delete_requires_confirm`, `read_only_blocks_insert`, `results_show_null_distinctly` (NULL rendered as a dimmed `NULL` badge, empty string as empty), `history_click_opens_tab`.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Manual check on the test cluster: tree lists the test tables; `select * from <table>` as admin shows the permission explanation with the grant snippet; after `SET` to a schema the user can read, results show. **This is the MVP the task asked for (connect via SSO, browse tables, run queries).** Tell the owner, in Spanish, with what was checked.
- [ ] **Step 6:** Write `docs/working-with-data.md` (editor, shortcuts, warnings list, history and where it is stored). Commit `feat(ui): schema tree, SQL editor, results and history`.

### Task 15: Table browser and editing (UI)

**Files:** Create `src/components/{TableBrowser,SaveDialog}.tsx`.

- Behaviour (spec 5.5): open table from the tree into a tab; pages of 200 rows; sort by header; per-column filter row; free `WHERE` box; "Show SQL"; edit cell (NULL toggle per cell, distinct from empty), add row, delete selected rows; staged changes highlighted; Save → `preview_changes` → dialog listing every statement and the row count (red with explicit "I understand, apply to production" confirm when production; one-click otherwise; split-into-batches note when >1 batch) → `apply_changes`; OCC retries shown from `db://occ-retry`, final conflict → "reload" action; tables without PK read-only with tooltip; copy as CSV/TSV/JSON/INSERT; export result to CSV via the Tauri dialog/fs plugins.
- [ ] **Step 1:** Failing tests: `save_dialog_lists_statements`, `production_save_needs_explicit_confirm`, `no_pk_table_is_read_only`, `null_toggle_differs_from_empty`, `copy_as_insert_quotes_values`.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Manual check on a scratch table (created and dropped during the check) or on the test app's table only with the owner's OK.
- [ ] **Step 6:** Extend `docs/working-with-data.md` (browsing, editing, production confirmation, batching, OCC). Commit `feat(ui): table browser with staged edits`.

### Task 16: Role wizard and "Check access" (UI)

**Files:** Create `src/components/RoleWizard.tsx`, `src/components/CheckAccess.tsx`.

- Behaviour (spec 5.7): steps — access (read-only / read-write) → IAM role (default from caller identity via `iam_role_arn`, fallback `sso_role_arn_guess`, editable) → role name → review of admin SQL and owner SQL → run admin SQL → owner step with three choices: "Run it for me" (console Lambda, function name editable, shows tinker code and output), "Copy tinker snippet", "Copy SQL" — plus "Let admin do it (temporary)" only when `admin_can_take_over` is true (Task 1 Step 5: works through role membership, no `SET ROLE`), with an explanation and explicit consent, revoking right after → test connection as the new role → offer to save it as a connection. "Remove role" flow in the right order. "Check access" table: each table × SELECT/INSERT/UPDATE/DELETE for the current role, with the exact grant for missing ones.
- [ ] **Step 1:** Failing tests: `wizard_shows_sql_before_running`, `remove_flow_orders_revokes_first`, `admin_takeover_hidden_unless_supported`, `check_access_lists_missing_grants`.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5:** Manual end-to-end on the test cluster with the owner's OK: create readonly role, run owner grants via console, connect as it, remove it, confirm the cluster is clean.
- [ ] **Step 6:** Write `docs/team-access.md` (the recipe, what each step does, permissions needed incl. `lambda:InvokeFunction`, removal). Commit `feat(ui): role wizard and access check`.

### Task 17: macOS build, README, wrap-up

**Files:** Modify `README.md`, `docs/development.md`, `src-tauri/tauri.conf.json` (bundle: `dmg`, `app`; minimum macOS 11; icons).

- [ ] **Step 1:** `npm run tauri build -- --target aarch64-apple-darwin` → `.app` and `.dmg` under `src-tauri/target/.../bundle/`. Record size. Unsigned is acceptable; document the Gatekeeper "Open anyway" step and that signing/notarisation needs the owner's Apple Developer ID (later).
- [ ] **Step 2:** README: what it is and why (the three walls), screenshots placeholder, install (from the `.dmg`, from source), first connection walkthrough, IAM permissions table (`dsql:DbConnectAdmin`, `dsql:DbConnect`, `dsql:ListClusters`, `dsql:GetCluster`, `dsql:ListTagsForResource`, `iam:GetRole` optional, `lambda:InvokeFunction` optional), what is stored locally and what is not, DSQL limitations with links to the Laravel driver, troubleshooting, licence MIT. Links to `docs/*.md`.
- [ ] **Step 3:** Full test run: `cargo test --workspace`, `npm test`, gated integration suite with the owner's confirmed profile. All PASS (report any skipped test and why).
- [ ] **Step 4:** Update the "Status" section of `CLAUDE.md` as the hand-over note (what is done, what is pending: signing, Windows/Linux, deep link from Arco Cloud) and commit `docs: README and macOS build`.
- [ ] **Step 5:** Ask the owner (in Spanish) for an explicit "si" before creating `multi-api/arco-dsql-studio` on GitHub (public) and pushing. Do not push before.
