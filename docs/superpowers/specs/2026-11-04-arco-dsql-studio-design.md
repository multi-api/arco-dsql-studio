# Arco DSQL Studio — design

Date: 2026-11-04. Status: draft for the owner's review.
Name: **Arco DSQL Studio** (repository `arco-dsql-studio`, public, MIT; see section 12).

This document is meant to be handed to another engineer (or another AI session) who will plan and build the app
without talking to the owner. Where something is the author's call rather than the owner's, it says so.

## 1. Why this exists

Arco deploys Laravel apps to AWS Lambda; Arco Cloud (the paid dashboard) creates Amazon Aurora DSQL databases in the
customer's own AWS account. **So far, no Arco user has managed to connect to their DSQL database to look at their
tables, list them, or change a row.** Generic clients (TablePlus, DBeaver, DataGrip, `psql`) technically work but
fail on three DSQL-specific walls:

1. **No passwords.** You connect with a short-lived IAM token (15 minutes). The `admin` role needs
   `dsql:DbConnectAdmin` and the admin token variant; any other role needs `dsql:DbConnect`, the normal token, and an
   `AWS IAM GRANT <role> TO '<IAM role ARN>'` mapping. Generic clients make you paste a fresh token by hand.
2. **The tables are not in `public`.** Each environment's tables live in a schema named like the app's DSQL user
   (for example `myapp_staging`). The app's role owns them. So `admin` gets
   `relation "users" does not exist` without `search_path`, and `permission denied for table users` until the owner
   grants access.
3. **You cannot sign in as the app's role.** It is mapped only to the app's Lambda execution role.

The app removes those walls: pick your AWS profile, pick your cluster, and you are looking at your tables.

### Success

- An Arco user who has never used DSQL opens the app, signs in with their AWS profile (or sets one up from inside the
  app), and sees their environment's tables with rows **in under two minutes, without reading docs**.
- They can query with SQL and edit rows in a spreadsheet-like grid.
- When DSQL says no (permissions, unsupported SQL, OCC conflicts), the app explains why in plain language and what to
  do next.

## 2. Goals, non-goals, anti-goals

**Goals (owner's)**
- A desktop database client made only for Aurora DSQL, in the spirit of DataGrip but far simpler.
- Both ways of working: **a SQL editor** and **a table browser that looks like a spreadsheet**, where you can filter,
  sort and **edit cells**.
- **Editing is allowed, production included, at the user's own risk.**
- **Independent of Arco Cloud.** It must work for anyone with an AWS account and a DSQL cluster, without an Arco Cloud
  account or the Arco CLI. Arco-specific conveniences (recognising Arco's schema naming) are a bonus, never a
  requirement.
- Works for users who already have an AWS profile on their machine (SSO or access keys) **and** for users who have
  never configured AWS on their computer.

**Non-goals (for the MVP)**
- Other databases (plain Postgres, Aurora Postgres, MySQL). DSQL only.
- Schema design tools (ER diagrams, visual table designer, migrations). Arco users change schema through Laravel
  migrations.
- Team features, cloud sync, sharing connections.
- Windows and Linux builds (later; Tauri makes them possible). **macOS first.**
- Anything that needs Arco Cloud to be running (a deep link from Cloud is an optional later item, section 9).

**Anti-goals (would make it a failure even if it "works")**
- Asking the user for a password or a token. If they ever have to paste a token, the app has failed.
- Storing AWS secrets anywhere of its own. Credentials stay in `~/.aws` (and the SSO cache) where the AWS CLI keeps
  them; the app only reads them through the AWS SDK.
- Opening on an empty `public` schema and leaving the user to guess.
- Showing raw PostgreSQL errors with no explanation.
- Silently changing production data. Edits in production are allowed but always shown and confirmed first.

## 3. Users

- **Arco users**: Laravel developers, comfortable with Laravel and Eloquent, often not with AWS IAM or Postgres
  roles. Some write SQL, some only want to browse tables.
- Two setups:
  - **Has an AWS profile** on the machine (most often IAM Identity Center / SSO, because the Arco CLI needs one to
    deploy; sometimes access keys).
  - **Has never configured AWS** on the computer (for example, only used Arco Cloud in the browser).

## 4. How it gets made (author's calls; the owner left the stack to the author)

- **Tauri 2** desktop app.
  - **Core in Rust:** AWS, tokens, Postgres connections, SQL analysis, local storage.
  - **UI in React + TypeScript** (Vite).
- Why not a pure-Rust GUI (egui/iced): a good SQL editor (highlighting, autocomplete) and a large editable results
  grid are much easier and better on the web stack.
- Why not Electron: Tauri gives a ~10 MB macOS bundle instead of ~150 MB, and the Rust core is where the AWS and
  Postgres work belongs anyway.
- Rust crates (author's call; the builder may swap equivalents):
  - `aws-config` with the `sso` feature, `aws-sdk-dsql` (ListClusters, GetCluster, and the `auth_token` token
    generator), `aws-sdk-sts` (caller identity), `aws-sdk-iam` (resolve the IAM role ARN, section 6.5),
    `aws-sdk-ssooidc` + `aws-sdk-sso` (in-app SSO sign-in and account/role listing, section 6.1).
  - `tokio-postgres` with TLS through `rustls` (`tokio-postgres-rustls` + `webpki-roots` or the native roots).
    **sslmode verify-full**: the endpoint certificate is checked against the hostname.
  - `sqlparser` (PostgreSQL dialect) for statement classification (section 6.6), with a fallback to keyword rules
    when parsing fails.
  - `rusqlite` for local app data (history, saved connections, settings).
- UI libraries (author's call): CodeMirror 6 with `@codemirror/lang-sql` for the editor (schema-aware autocomplete),
  a virtualized grid (TanStack Table + TanStack Virtual, or Glide Data Grid) for results and table data. Tailwind is
  fine.
- **Frontend ↔ core:** Tauri commands (request/response) and Tauri events (progress, token refresh, query
  cancellation, OCC retries). The frontend never sees AWS credentials or tokens.

## 5. Experience, screen by screen

### 5.1 First launch: "Who are you on AWS?"
- The app reads `~/.aws/config` and `~/.aws/credentials` (respecting `AWS_CONFIG_FILE` /
  `AWS_SHARED_CREDENTIALS_FILE`) and lists profiles: name, type (SSO, access keys, role assumption, process), and for
  SSO the account and role.
- For an SSO profile whose session expired, a **Sign in** button runs the SSO device flow (section 6.1) and opens the
  browser, exactly like `aws sso login`, then updates the shared SSO token cache so the AWS CLI also sees the session.
- **No AWS on this machine?** A "Set up AWS access" wizard:
  1. Ask for the SSO start URL (the "AWS access portal URL") and the SSO region. A help link explains where to find
     it.
  2. Run the device flow in the browser.
  3. List the accounts and roles the user can reach (SSO `ListAccounts` / `ListAccountRoles`).
  4. Pick one; the app **writes a new profile** (and `sso-session`) to `~/.aws/config` after showing the exact lines
     it will add and asking for confirmation. It never edits or removes existing lines.
  - Access-key setup is **not** offered in the wizard (author's call: less safe, and the app never handles secrets).
    Users who already have access-key profiles can use them.
- After picking a profile the app shows "Signed in as `<assumed role>` in account `<id>`" (STS GetCallerIdentity).

### 5.2 Cluster picker
- For the chosen profile, list DSQL clusters in every region where DSQL is available (call ListClusters per region
  in parallel; regions where the call fails with access denied or "not available" are skipped quietly). Show:
  identifier, region, status, endpoint, tags (Arco Cloud tags, if any, show the app and environment name; the builder
  should read them from the cluster's tags, not assume them).
- Remember the last region(s) used so the next launch is instant; a "Scan all regions" action refreshes.
- A manual entry ("endpoint + region") for clusters the profile cannot list but can connect to.

### 5.3 Connect: pick who you connect as
- Two choices, explained in one line each:
  - **admin**: needs `dsql:DbConnectAdmin`. Can see everything; cannot read the app's tables until granted.
  - **A custom role** (for example `readonly` or `team_rw`): needs `dsql:DbConnect` and an `AWS IAM GRANT` mapping to
    the user's IAM role.
- The app tries the connection. If it fails, it explains which of the above is missing (section 6.7).
- After connecting, **pick the schema**: list non-system schemas. If there is exactly one, use it. If one or more
  match Arco's naming (`<app>_<environment>`), preselect the best match. The chosen schema becomes `search_path` for
  the session (`SET search_path = "<schema>"`), so `select * from users` just works.
- A saved **connection** = profile + region + cluster + role + schema + a "production" flag + a "read-only" toggle.
  Saved locally (no secrets). One-click reconnect from the sidebar.

### 5.4 Main window
- Left: **schema tree** — schemas → tables, views, sequences → columns (type, nullable, default, PK), indexes.
  Table owner and the current role's privileges on it (SELECT / INSERT / UPDATE / DELETE) are visible on hover, with a
  lock icon on tables the role cannot read.
- Centre: tabs. Each tab is either a **SQL editor** or a **table**.
- Top bar: connection name, role, schema, a **PRODUCTION** badge in red when the flag is on, read-only indicator.

### 5.5 Table browser (spreadsheet view)
- Opens a table with paged rows (server-side `LIMIT/OFFSET` or keyset paging on the PK; default page 200 rows).
- Sort by clicking a column header, filter per column (equals, contains, is null, >, <) and a free-text `WHERE`
  box. Filters and sorts become SQL the user can see ("Show SQL").
- **Editing:**
  - Edit a cell, add a row, delete rows. Changes are staged (highlighted), not sent.
  - **Save** shows the exact `UPDATE` / `INSERT` / `DELETE` statements and the number of affected rows; in production
    the dialog is red and needs an explicit confirm (owner: "editar bajo su responsabilidad" — allowed, but always
    shown first). Outside production, one click on Save.
  - Edits target rows by primary key. Tables without a primary key are read-only in the grid (explained in a
    tooltip).
  - All staged changes run in one transaction. DSQL limits how many rows a transaction can modify (documented as
    3,000 rows at the time of writing; the builder must confirm the current quota) — the app splits larger saves into
    batches and says so before running.
  - On SQLSTATE `40001` (OCC conflict), retry the transaction (section 6.4); if it still fails, say that someone else
    changed the same rows and offer to reload.
- Copy cells/rows as CSV, TSV, JSON, or `INSERT` statements. Export a query result to CSV.

### 5.6 SQL editor
- Highlighting, schema-aware autocomplete (tables/columns of the current schema), run the statement under the cursor
  or the selection, run all.
- Results grid below (read-only for arbitrary queries in the MVP), with row count, duration, and messages.
- Cancel a running query.
- **History**: every statement run, with connection, time, duration, rows, success/error; searchable; click to reopen.
- **Before running**, the statement is classified (section 6.6):
  - DSQL will reject it → warning with the reason and the DSQL alternative; user can still run it.
  - It writes data or changes schema and the connection is production → confirmation.
  - The connection is read-only → writes are blocked with an explanation.

### 5.7 Role wizard ("Give my team access")
For DSQL's real problem: the app's tables belong to the app's role, and nobody can sign in as it.
- Guided steps, based on the recipe tested end to end on 2026-11-04 (section 6.5): choose **read-only** or
  **read-write**, choose the IAM role to map (defaults to the role behind the current profile, ARN resolved with the
  path included), name the role.
- Steps the app can run itself (connected as `admin`) are run with a preview of the SQL. Steps that only the tables'
  owner can run (`GRANT SELECT ...` per table) are shown as a ready-to-paste snippet for `vendor/bin/arco tinker` or
  Arco Cloud's **Run command**, plus a plain-SQL variant for non-Arco users. For Arco Cloud apps, a "Run it for me"
  button can invoke the environment's console Lambda with the user's own profile (section 6.5). The spike found that
  `admin` can grant directly through temporary membership in the app's role (section 12), when `admin` holds ADMIN
  OPTION on it; that path is offered as one more choice, with the user's explicit consent.
- A **"Remove role"** flow that revokes in the right order before `DROP ROLE`.
- A **"Check access"** view: for the current role, which tables it can read/write and which it cannot, with the
  exact grant needed.

## 6. How the core works

### 6.1 AWS credentials and SSO
- Load credentials with `aws-config` for the chosen profile (supports SSO with `sso-session`, legacy SSO, assume-role
  chains, `credential_process`, access keys).
- If the SSO token is missing or expired, the SDK fails; the app catches that and runs the device authorization flow
  with `aws-sdk-ssooidc` (`RegisterClient` → `StartDeviceAuthorization` → open the verification URL with the user
  code → poll `CreateToken`), then writes the token to the standard SSO cache (`~/.aws/sso/cache/<sha1>.json`) in the
  same format the AWS CLI uses, so both tools share the session. The builder must match the CLI's cache format
  exactly (key = SHA-1 of the session name for `sso-session` profiles, or of the start URL for legacy ones).
- The app never writes access keys and never creates IAM users, roles or policies.

### 6.2 Tokens
- `admin` → admin token (`DbConnectAdmin`); any other role → normal token (`DbConnect`). Generated locally with the
  SDK's signer (no network call), expiry 15 minutes.
- A token only matters when a connection is opened. Each new connection gets a fresh token; the pool (section 6.3)
  opens new connections as needed, so tokens are always "refreshed" by construction. If the underlying credentials
  (SSO session) expire, the next connect fails with a clear "Sign in again" prompt.

### 6.3 Postgres connections
- `tokio-postgres` over TLS, `sslmode=verify-full`, database `postgres`, user = role name, password = token.
- A small pool per saved connection (for example 2–4 connections: one for the tree/metadata, others for tabs).
  Every connection runs `SET search_path` on open.
- **DSQL closes connections after 60 minutes.** Recycle connections before 55 minutes and reconnect transparently on
  "connection closed" errors when no transaction is open.
- **Prepared statements risk:** the Laravel driver found that libpq 17+ closes server-side prepared statements with a
  protocol message DSQL rejects (aborting the transaction with `25P02`). `tokio-postgres` prepares named statements and
  may close them the same way. The builder must verify this first (section 8). Expected approach: run user SQL with
  the **simple query protocol** (`simple_query`), which also handles multi-statement scripts and returns every value
  as text — good for a grid; use typed/unnamed statements (`query_typed`) or careful literal quoting for grid edits,
  whichever the spike proves safe.
- Cancel: DSQL ignores cancel requests (spike, `docs/spike-findings.md`), so the app sends one and then closes the
  connection; the pool opens a new one. The UI says the statement may still finish on the server.

### 6.4 OCC retries
- SQLSTATE `40001` on commit or statement → retry the whole transaction up to 3 times with jittered backoff, only for
  transactions the app built itself (grid saves, wizard steps). For arbitrary SQL typed by the user, the app does not
  replay the script silently: it shows "conflict, retry?" with a one-click retry (author's call: replaying a user's
  multi-statement script could duplicate side effects).

### 6.5 The access recipe (reference)
Tested end to end on 2026-11-04 on a Cloud-managed cluster with an SSO role whose ARN has a path. Source:
`arco-cloud` docs, `docs/databases.md`, "Querying the database yourself".

Create (as `admin`):
```sql
CREATE ROLE readonly WITH LOGIN;
AWS IAM GRANT readonly TO 'arn:aws:iam::<account>:role/<path/role-name>';
GRANT USAGE ON SCHEMA "<schema>" TO readonly;
ALTER ROLE readonly SET search_path = "<schema>";
```
Then, **as the tables' owner (the app's role, only usable from the app's Lambda)**, per table:
```sql
GRANT SELECT ON "<table>" TO readonly;            -- read-write: SELECT, INSERT, UPDATE, DELETE
```
Tables added by later migrations need the grant again.

Remove — revoke first, `DROP ROLE` refuses otherwise:
1. As the owner: `REVOKE SELECT ON "<table>" FROM readonly;` per table.
2. As `admin`: `AWS IAM REVOKE readonly FROM '<arn>'; REVOKE USAGE ON SCHEMA "<schema>" FROM readonly; DROP ROLE readonly;`

**Running the owner-side grants from the desktop app (Arco Cloud apps only).** On 2026-11-04 the owner-side
`GRANT SELECT` was run by invoking the environment's console Lambda, `<app>-<environment>-console`, alias `live`, with
the payload `{"command":"tinker --execute=\"...\""}` — the same thing Arco Cloud's Run command does. (The local Arco CLI
refused because the app's `arco.php` does not declare the console; Cloud configures that function.) So, when the
user's IAM role has `lambda:InvokeFunction` on that function, the wizard can offer "Run it for me" instead of the
snippet: show the exact tinker code, invoke, show the output. This uses the user's own AWS profile, not Arco Cloud, so
the app stays independent; for non-Arco clusters (no console function) the snippet stays. The builder must derive the
function name from the schema/role name and let the user confirm or correct it.

Facts the wizard relies on:
- The IAM role ARN must be the IAM role's ARN (`arn:aws:iam::<acct>:role/<path>/<name>`), not the STS assumed-role
  ARN. For SSO roles the path is `aws-reserved/sso.amazonaws.com/<region>/`. Resolve it with
  `iam:GetRole(<role name from the assumed-role ARN>)`; if the user lacks `iam:GetRole`, build it from the known SSO
  path pattern and let the user correct it.
- `sys.iam_pg_role_mappings` lists existing mappings.
- `admin` can grant USAGE on the schema because Arco Cloud created the schema as `admin`. Other setups may differ.
- In DSQL a non-admin role cannot create objects in `public`.

### 6.6 Statement classification (warnings)
Parse with `sqlparser` (PostgreSQL dialect); fall back to keyword rules. Classes:
- **Rejected by DSQL** (warn, explain, give the alternative; list from the `multiapi/laravel-aurora-dsql` README,
  verified on a live cluster):
  - `TRUNCATE` → `DELETE FROM`.
  - `ALTER TABLE ... ADD` PRIMARY KEY / FOREIGN KEY / UNIQUE / CHECK → declare in `CREATE TABLE`; unique →
    `CREATE UNIQUE INDEX ASYNC`.
  - `ADD COLUMN` with `DEFAULT`, `NOT NULL` or a FK → add nullable, set default, backfill.
  - `ALTER COLUMN ... TYPE`, `SET NOT NULL` → new table + copy + rename.
  - `SAVEPOINT`, `ROLLBACK TO`, `RELEASE`.
  - `CREATE TEMP/TEMPORARY TABLE`.
  - `CREATE TRIGGER`, `LANGUAGE plpgsql` functions/`DO` blocks.
  - `CREATE EXTENSION`.
  - advisory locks (`pg_advisory_lock` & co.), `FOR SHARE`.
  - full-text search configs other than `simple`.
  - `CREATE DATABASE`.
  - `CREATE INDEX` without `ASYNC`, on any table (the spike found DSQL rejects it even on an empty table:
    `0A000 unsupported mode. please use CREATE INDEX ASYNC.`) → `CREATE INDEX ASYNC`.
  - more than one DDL statement in a transaction.
- **Writes data** (INSERT/UPDATE/DELETE/MERGE/COPY) and **changes schema** (DDL, GRANT/REVOKE, role commands) →
  production confirmation; blocked when read-only.
- **Read** → runs directly.
The rule list lives in one data file (Rust or JSON) with id, pattern, message, alternative and a docs link, so it is
easy to update as DSQL changes.

### 6.7 Explaining errors
Map errors to plain-language explanations with a "how to fix" action:
- `relation "x" does not exist` while `x` exists in another schema → "Your session is looking in `<search_path>`;
  the table is in `<schema>`. Switch schema?" (one click).
- `permission denied for table x` → "Table `x` belongs to `<owner>`. Your role `<role>` has no SELECT on it. Only the
  owner can grant it." + the grant snippet (section 5.7).
- `permission denied for schema` → missing `GRANT USAGE`.
- Connection rejected / authentication failed → which permission is missing:
  - `admin` → `dsql:DbConnectAdmin` on the cluster ARN.
  - other roles → `dsql:DbConnect`, or no `AWS IAM GRANT` mapping for this IAM role (show the ARN the app used).
  - Include the caller identity.
- Expired SSO session → "Sign in again" button.
- `40001` → OCC explanation (section 6.4).
- `25P02` → "The transaction was aborted by an earlier error; roll back."
- Anything unknown → the raw message, SQLSTATE, and a "copy details" button.

## 7. Local data
- App data directory (Tauri's app data dir): `studio.sqlite` with saved connections, query history, settings.
- **No secrets** are stored: no tokens, no keys. SSO tokens live only in the standard `~/.aws/sso/cache`.
- History is local only; a setting turns it off and a button clears it.

## 8. Risks to resolve first (spike before building screens)
Use the owner's end-to-end test account and DSQL cluster (us-east-1). The repository is public, so the account ID,
cluster ID, profile name and role ARNs are **never committed**: tests and scripts read them from env vars
(`DSQL_HOST`, `DSQL_REGION`, `AWS_PROFILE`), and the owner gives the concrete values to whoever builds the app. Confirm the profile and account with the owner before any AWS call.
Destructive AWS actions are the owner's; SQL experiments must clean up after themselves (drop test roles and tables).
1. `tokio-postgres` + rustls connects with an admin token and with a custom-role token (verify-full).
2. Prepared/extended protocol: does `query`, `query_typed` or statement drop break the transaction on DSQL (the
   `25P02` problem)? Decide the query strategy from the result.
3. Query cancellation works or needs a connection close.
4. Catalog queries the schema tree needs (`pg_namespace`, `pg_class`, `pg_attribute`, `pg_index`,
   `information_schema.table_privileges`, `has_table_privilege`) work on DSQL and are fast enough.
5. Can `admin` grant table privileges on tables it does not own, for example by being granted membership in the app's
   role (`GRANT <app_role> TO admin; SET ROLE <app_role>`)? If yes, the wizard can do the whole recipe itself; if no,
   it keeps the owner-side snippet. **Do not leave this membership in place on a real customer cluster without telling
   the user**; on the test cluster, revoke it after the test.
6. `aws-config` SSO with `sso-session` profiles, and writing the CLI-compatible SSO cache.

Already known from a real test on that cluster (2026-11-04), no need to repeat:
- An Arco test app (staging environment) is deployed there: its schema has 10 tables, all owned by the app's role
  (same name as the schema).
- As `admin`: `relation "users" does not exist` without `search_path`; `permission denied for table users` with it.
- `CREATE ROLE ... WITH LOGIN`, `AWS IAM GRANT` to an SSO role ARN with its path, `GRANT USAGE ON SCHEMA` and
  `ALTER ROLE ... SET search_path` all work as `admin`.
- Owner-side `GRANT SELECT` works through the console Lambda (section 6.5). As the new role: `search_path` applies by
  itself, `SELECT` works, `INSERT`/`DELETE`/`CREATE TABLE` answer `permission denied`.
- Removal order: table `REVOKE` (owner) → schema `REVOKE` (admin) → `AWS IAM REVOKE` → `DROP ROLE`.
- The cluster was left clean (the test role and its mapping were removed).

Write the findings to `docs/spike-findings.md` in this repository.

## 9. Later (not MVP)
- **"Open in DSQL Studio" from Arco Cloud**: a deep link `arco-dsql-studio://connect?region=..&cluster=..&schema=..&role=..`
  (Tauri deep-link plugin). The app still uses the user's own AWS profile; the link carries no credentials. Needs a
  change in Arco Cloud (another repository, another session).
- Editable results for arbitrary single-table queries; EXPLAIN view; saved queries; Windows and Linux builds; signed
  and notarized macOS releases (needs the owner's Apple Developer ID); auto-update.

## 10. MVP and definition of done
The MVP is done when, on macOS:
1. With the owner's SSO test profile, the app lists the clusters, connects to the real cluster as `admin` and as a
   custom role, and opens in the environment's schema.
2. The schema tree shows the tables, and the table browser shows paged rows with sort and filter.
3. The SQL editor runs queries, shows results, keeps history, warns about DSQL-unsupported statements, and explains
   `permission denied` / `relation does not exist`.
4. Grid edits (update/insert/delete by PK) save in a transaction with preview and production confirmation, with OCC
   retry.
5. The "no AWS configured" wizard creates an SSO profile in `~/.aws/config` after confirmation.
6. The role wizard creates and removes a read-only role on the test cluster (owner-side grants via snippet, or
   automatically if spike item 5 allows).
7. `npm run tauri build` produces a `.app`/`.dmg` for Apple Silicon (unsigned is acceptable for the MVP).
8. README (what it is, install, first connection, required IAM permissions, troubleshooting), this spec, the
   implementation plan, and `docs/spike-findings.md` are in the repository. Every behaviour above is documented
   (the owner wants everything documented).
- Tests:
  - Rust unit tests for statement classification, error mapping, ARN resolution, SSO cache key, and SQL generation
    for grid edits (quoting!).
  - An integration test suite against a DSQL cluster, gated by env vars (like the Laravel driver's
    `DSQL_HOST` / `DSQL_PROFILE`).
  - Frontend component tests where useful.

## 11. Left to the builder
Exact crates and UI libraries (within section 4's direction), visual design, pool sizes, paging strategy, keyboard
shortcuts, file layout, how the classification rules are encoded, and the plan's task breakdown and order (the spike
in section 8 comes first).

## 12. Decisions

Decided by the owner (2026-11-04):
- Name **Arco DSQL Studio**, repository `arco-dsql-studio`.
- **Public** repository under the `multi-api` GitHub organisation, **MIT** licence. So nothing account-specific
  (account IDs, cluster IDs, ARNs, profile names) is committed; see section 8.

Author's call, pending the owner's confirmation: spike item 5 showed that `admin` can take the app role's privileges
on DSQL. `SET ROLE` is not supported, but `GRANT <app_role> TO admin` alone lets `admin` read the tables and grant on
them; it needs `admin` to hold ADMIN OPTION on the app role (true when `admin` created it). The app does it **only when
the user accepts it at that moment**, with a clear explanation, runs `REVOKE <app_role> FROM admin` right after the
grants (and again on disconnect if something failed in between). It is never done silently and never left in place.

Publishing:
- Nothing gets created on GitHub, pushed or published without the owner's explicit "si".

## 13. Working rules for whoever builds this
- Talk to the owner in Spanish; code and docs in English.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; PR bodies end with
  `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.
- Confirm the AWS profile and account before any AWS call; never create credentials; destructive AWS actions are the
  owner's.
- Do not touch the `arco-cloud` repository from this project.
