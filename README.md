# Arco DSQL Studio

A desktop database client made only for [Amazon Aurora DSQL](https://aws.amazon.com/rds/aurora/dsql/): sign in with
your AWS profile (IAM Identity Center / SSO included), pick your cluster, and you are looking at your tables. No
passwords, no pasted tokens.

> **Status: early development.** You can sign in with an AWS profile (or set one up), find your clusters and connect;
> the workspace (tables, SQL editor, results) is next. How connecting works and which IAM permissions it needs:
> [`docs/connecting.md`](docs/connecting.md). To build from source: [`docs/development.md`](docs/development.md).

## Why

Generic Postgres clients technically work with DSQL, but users hit three walls:

1. **No passwords.** You connect with a 15-minute IAM token (`admin` needs `dsql:DbConnectAdmin`; other roles need
   `dsql:DbConnect` plus an `AWS IAM GRANT` mapping).
2. **The tables are not in `public`.** Apps deployed with [Arco](https://github.com/multi-api) keep each
   environment's tables in its own schema, owned by the app's role, so `admin` gets `relation does not exist` or
   `permission denied`.
3. **You cannot sign in as the app's role**: it is mapped only to the app's Lambda role.

Arco DSQL Studio handles the tokens, opens in the right schema, explains DSQL's errors and limits in plain language,
and has a wizard to give your team a read-only or read-write role. It works with any DSQL cluster; it does not need
Arco Cloud.

## Planned features (MVP)

- AWS profiles from `~/.aws` (SSO, access keys, role chains) and an in-app SSO setup for people who never configured
  AWS.
- Cluster discovery across regions; connect as `admin` or a custom role; `search_path` set for you.
- Schema tree, SQL editor with history, results grid, spreadsheet-like table browser with editing (previewed and
  confirmed in production).
- Warnings before statements DSQL rejects, automatic OCC (`40001`) retry for the app's own writes, clear explanations
  of permission and schema errors.
- Role wizard (create and cleanly remove a role mapped to an IAM role).
- macOS first (Tauri 2: Rust core + React/TypeScript UI); Windows and Linux later.

## For whoever builds this

Read, in order:

1. [`CLAUDE.md`](CLAUDE.md) — working rules (language, AWS safety, commits, what never goes in this public repo).
2. [Design spec](docs/superpowers/specs/2026-11-04-arco-dsql-studio-design.md) — what to build and why.
3. [Implementation plan](docs/superpowers/plans/2026-11-04-arco-dsql-studio-mvp.md) — 18 tasks; Task 1 is a spike
   against a real cluster and comes before any screen.

## Licence

MIT — see [LICENSE](LICENSE).
