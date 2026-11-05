# DSQL spike findings

Date: 2026-11-04. Plan Task 1, spec section 8. Reproduce with `crates/dsql-core/examples/spike.rs`
(`setup`, `run`, `teardown`, `index`; environment variables in its header comment).

## Where it ran

- A **scratch DSQL cluster created for these tests** in us-east-1 (PostgreSQL 16 compatible), not an Arco app's
  cluster. The owner chose this so nothing touches a real app's tables or roles.
- Signed in with an IAM Identity Center (SSO) profile whose role has administrator access.
- `setup` builds a stand-in for an Arco app: role `spike_owner` (mapped to the same IAM role so the spike can sign in
  as it) owns schema `spike_app`'s tables `users`, `orders` and `"Order Items"` (column `"we""ird"`). `admin` created
  the role and the schema, as Arco Cloud does.
- The real Arco console Lambda was **not** used. The owner-side grant ran as `spike_owner` directly. "Run it for me"
  (spec 6.5) is still unverified from Rust; it was verified by hand on 2026-11-04 (spec section 8).
- `teardown` dropped every table, mapping, schema and role; `sys.iam_pg_role_mappings` and `pg_roles like 'spike%'`
  were empty afterwards.

## Results

### 1. TLS connection with IAM tokens

- `tokio-postgres` 0.7 + `tokio-postgres-rustls` 0.14 + `rustls` 0.23 (ring provider) + `webpki-roots` connects as
  `admin` (admin token) and as custom roles (normal token). `select version()` → `PostgreSQL 16`.
- DSQL's certificate chains to a public root in `webpki-roots`; no extra CA bundle is needed.
- Hostname verification works: connecting to the endpoint's IP address fails the TLS handshake. rustls always checks
  the chain and the hostname, so `SslMode::Require` with this connector is `verify-full`.
- `aws_sdk_dsql::auth_token::AuthTokenGenerator` with `db_connect_admin_auth_token` / `db_connect_auth_token` and
  `expires_in(900)` works with an `sso-session` profile loaded by `aws-config` (`sso` feature).
- Each statement round trip from the test machine took ~125–190 ms (network latency dominates).
- The AWS SDK's HTTP client pulls in `aws-lc-rs`, which compiles C code (the Windows build machine had MSVC and
  CMake). The spike's own Postgres TLS uses rustls' `ring` provider.

### 2. Extended protocol (the `25P02` risk)

All inside explicit transactions, each followed by `select 1` and `COMMIT`:

| Call | Result |
| --- | --- |
| `client.query("select $1::int", ..)` (named statement, closed afterwards) | OK, transaction intact |
| `client.prepare(..)`, query, drop the `Statement` (explicit `Close` message) | OK, transaction intact |
| `client.query_typed(..)` (unnamed statement) | OK, transaction intact |
| `simple_query` with two statements | OK, both result sets returned |

The libpq 17 problem does not happen with `tokio-postgres`. An error inside a transaction gives the usual `25P02` on
the next statement until `ROLLBACK`.

**Decision:** `exec` uses `simple_query` (multi-statement scripts, every value as text). `simple_query` keeps `NULL`,
`''` and `'NULL'` distinct (`None`, `Some("")`, `Some("NULL")`). Grid edits may use `query_typed`; literal SQL built
by `quote.rs` stays a valid alternative.

### 3. Query cancellation

- `CancelToken::cancel_query` over TLS returns `Ok`, but **DSQL ignores the cancel request**: `select pg_sleep(30)`
  ran the full 30 s and returned normally. The connection stayed usable.
- AWS documentation does not mention cancel requests.

**Decision:** "Cancel" drops the session's connection (the pool opens a new one) and tells the user the statement may
still finish on the server. The app still sends a cancel request first, in case DSQL starts honouring it.

### 4. Catalog queries

All work as `admin` on DSQL, 130–220 ms each, mostly network:

- Schemas: `pg_namespace` with `pg_get_userbyid(nspowner)`. `public` is owned by `pg_database_owner`.
- Tables: `pg_class` + `pg_namespace` + `has_table_privilege(oid, 'SELECT'|'INSERT'|'UPDATE'|'DELETE')`. Owner via
  `pg_get_userbyid(relowner)`.
- Columns: `pg_attribute` + `format_type` + `pg_attrdef`/`pg_get_expr`.
- Indexes: `pg_index` + `pg_class`.
- `sys.iam_pg_role_mappings` and `pg_roles.rolconfig` (a role's default `search_path`) are readable by `admin`.

Two surprises that change Task 6:

- **DSQL primary-key indexes cover every column.** `pg_index.indkey` of `users_pkey` lists `id, name, email`
  (`indnatts = 3`, `indnkeyatts = 1`). The key columns are the first `indnkeyatts` entries of `indkey`. A query using
  `attnum = any(indkey)` returns every column as the "primary key"; that would make grid edits target the wrong rows.
- `information_schema.table_privileges` returned **no rows** for `admin` (it only lists grants that involve the
  current role). Use `has_table_privilege(<role>, oid, <privilege>)` instead; it works for other roles too.

### 5. Can `admin` take the tables' owner's privileges?

- `SET ROLE` is **not supported**: `0A000 setting configuration parameter "role" not supported`.
- But `GRANT spike_owner TO admin` works, and membership **inherits** privileges: without `SET ROLE`, `admin` could
  `select` from the owner's tables and run `GRANT SELECT ON spike_app.orders TO spike_readonly`, and the grant took
  effect for that role.
- `REVOKE spike_owner FROM admin` works: on a new connection, `admin` gets `permission denied for table users` again.
- `pg_auth_members` keeps one row after the `REVOKE`: the membership PostgreSQL 16 gives the role's creator
  (`grantor = dbowner`, `admin_option = t`, `inherit_option = f`, `set_option = f`). That row is what allows `admin` to
  grant the role to itself. So **it works when `admin` created the app's role**; check
  `pg_auth_members.admin_option` before offering it. Not yet checked on an Arco Cloud cluster; the owner must OK even
  that read-only check.

**Decision:** the "Let admin do it (temporary)" path is possible, without `SET ROLE`:
`GRANT "<app_role>" TO admin` → grants → `REVOKE "<app_role>" FROM admin`. Spec section 12 still applies: only
with the user's explicit consent at that moment, never left in place.

### 6. Custom role with the spec 6.5 recipe

- As `admin`: `CREATE ROLE ... WITH LOGIN`, `AWS IAM GRANT` to an SSO role ARN with its path,
  `GRANT USAGE ON SCHEMA`, `ALTER ROLE ... SET search_path` all work.
- As the owner: `GRANT SELECT ON <schema>.<table> TO <role>` works.
- As the new role (normal token): `current_schema()` is the schema with no `SET` needed. `SELECT` works.
  `INSERT`/`DELETE` → `42501 permission denied for table users`. `CREATE TABLE` → `42501 permission denied for schema
  spike_app`, and in `public` → `42501 permission denied for schema public`.
- `DROP ROLE` before the table `REVOKE`s → `2BP01 role "…" cannot be dropped because some objects depend on it`. The
  order owner `REVOKE` → `AWS IAM REVOKE` → `REVOKE USAGE` → `DROP ROLE` works.
- Without a schema `search_path`, `admin` gets `42P01 relation "users" does not exist`; with it,
  `42501 permission denied for table users`. The same codes the error explanations (Task 9) need.
- An SSO role whose SSO region is `us-east-1` has the IAM ARN
  `arn:aws:iam::<account>:role/aws-reserved/sso.amazonaws.com/<role name>`, with **no region segment** (confirmed with
  `iam:GetRole`). Pin it in Task 3's `sso_role_arn_guess` tests; other SSO regions add `<region>/`.

### 7. DSQL limits and unsupported statements

From the AWS docs (read 2026-11-04):

| Limit | Value | Source |
| --- | --- | --- |
| Rows modified per transaction | 3,000 (`transaction row limit exceeded`) | [Quotas](https://docs.aws.amazon.com/aurora-dsql/latest/userguide/CHAP_quotas.html) |
| Data modified per write transaction | 10 MiB | [Quotas](https://docs.aws.amazon.com/aurora-dsql/latest/userguide/CHAP_quotas.html) |
| Transaction duration | 5 minutes (`transaction age limit of 300s exceeded`) | [Quotas](https://docs.aws.amazon.com/aurora-dsql/latest/userguide/CHAP_quotas.html) |
| Connection duration | 60 minutes | [Quotas](https://docs.aws.amazon.com/aurora-dsql/latest/userguide/CHAP_quotas.html) |
| Query memory | 128 MiB per transaction (`53200`) | [Quotas](https://docs.aws.amazon.com/aurora-dsql/latest/userguide/CHAP_quotas.html) |
| DDL per transaction | 1, and DDL and DML in separate transactions | [Unsupported features](https://docs.aws.amazon.com/aurora-dsql/latest/userguide/working-with-postgresql-compatibility-unsupported-features.html) |

Measured on the cluster:

- `CREATE INDEX` without `ASYNC` is **always rejected**, on an empty table too:
  `0A000 unsupported mode. please use CREATE INDEX ASYNC.` `CREATE INDEX ASYNC` returns a job id.
  ([docs](https://docs.aws.amazon.com/aurora-dsql/latest/userguide/working-with-create-index-async.html))
- `TRUNCATE` → `0A000 unsupported statement: Truncate`.
- `SAVEPOINT` → `0A000 unsupported transaction statement: SAVEPOINT`.

The list of regions where DSQL is available was not settled here; Task 4 takes it from the AWS docs.

## Changes to the plan and spec

- Task 5 / spec 6.3: cancel = drop the connection (cancel requests are ignored).
- Task 6: primary key from the first `indnkeyatts` entries of `indkey`; privileges with `has_table_privilege`, not
  `information_schema.table_privileges`.
- Task 8 / spec 6.6: rule `index_without_async` fires on every `CREATE INDEX` without `ASYNC`.
- Task 10: `MAX_ROWS_PER_TX = 3000`; also keep each batch under 10 MiB and 5 minutes. `query_typed` is safe.
- Task 11 / Task 16 / spec 5.7 and 12: the admin takeover works through role membership, without `SET ROLE`.
