# Arco DSQL Studio

A desktop database client made only for [Amazon Aurora DSQL](https://aws.amazon.com/rds/aurora/dsql/): sign in with
your AWS profile (IAM Identity Center / SSO included), pick your cluster, and you are looking at your tables. No
passwords, no pasted tokens.

> **Status: 0.1, early.** Everything below works and is tested, but the app has no signed release yet: build it from
> source or use an unsigned build (see [Install](#install)). macOS first; Windows builds from source too.

<!-- Screenshots: welcome screen, workspace with the schema tree and results, the role wizard. -->

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

## What it does

- **Sign in with what you already have:** the profiles in `~/.aws` (SSO, access keys, role chains), or set up SSO in
  the app if you never configured AWS. Tokens are generated and renewed for you.
- **Find your clusters** across regions, or paste an endpoint. Connect as `admin` or as your own role; the schema is
  picked for you.
- **Work with data:** schema tree with a lock on tables you cannot read, SQL editor with autocomplete and history,
  results grid, and a table browser you can sort, filter and edit like a spreadsheet. Edits are previewed, batched
  under DSQL's limits and retried on optimistic-concurrency conflicts (`40001`).
- **Before a statement runs** the app warns about what DSQL rejects (`TRUNCATE`, foreign keys, more than one DDL per
  transaction, …) and asks before writes on a connection marked production. Errors come with a plain explanation and
  the fix (often the exact `GRANT`).
- **Team access:** a wizard creates a role mapped to an IAM role with read-only or read-write grants, tests it and
  saves it as a connection; another removes it in the order DSQL needs. **Check my access** shows what the current
  role can do on each table.

Guides: [connecting](docs/connecting.md), [working with data](docs/working-with-data.md),
[team access](docs/team-access.md).

## Install

### From a build (macOS)

Open the `.dmg` and drag **Arco DSQL Studio** to Applications. The build is not signed or notarised yet, so the first
time macOS says it cannot check it: open **System Settings → Privacy & Security**, find the message about Arco DSQL
Studio and click **Open Anyway** (or right-click the app in Finder → **Open**). Signing needs an Apple Developer ID and
is planned.

### From source

You need Rust (stable), Node.js 22 or later and Tauri's prerequisites (Xcode command line tools on macOS; the
Microsoft C++ Build Tools and WebView2 on Windows). Then:

```sh
npm install
npm run tauri build      # macOS: .app and .dmg under target/release/bundle/
npm run tauri dev        # or run it with hot reload
```

Details, tests and the layout of the code: [`docs/development.md`](docs/development.md).

## First connection

1. Open the app. Your AWS profiles are listed; pick one and **Sign in** (SSO profiles open the browser to approve).
   No profile? **Set up AWS access** walks you through SSO and shows the exact lines it will add to `~/.aws/config`.
2. Pick your cluster (the app searches the regions you choose) or **enter an endpoint**.
3. Connect as **admin** or as **a custom role**, pick the schema, and decide whether this is production (writes then
   need confirmation) or read-only.
4. **Save** the connection to reopen it with one click next time.

As `admin` on an Arco app you will see the app's tables with a lock: `admin` cannot read them until their owner grants
it. Use **Access… → Give the team access** to create a role of your own ([team access](docs/team-access.md)).

## IAM permissions

| Action | Needed for |
| --- | --- |
| `dsql:ListClusters`, `dsql:GetCluster` | Listing clusters and their tags (optional: you can enter an endpoint) |
| `dsql:DbConnectAdmin` | Connecting as `admin` |
| `dsql:DbConnect` | Connecting as a custom role |
| `iam:GetRole` | Showing your IAM role's exact ARN in explanations and the role wizard (optional) |
| `lambda:InvokeFunction` | **Run it for me** in the role wizard, on an Arco app's console function (optional) |

The app never creates IAM users, roles, policies or access keys.

## What is stored on your machine

- In the app's data directory, `arco-dsql-studio.sqlite`: saved connections (profile name, region, endpoint, role,
  schema, flags), query history (can be turned off) and settings. No passwords, tokens or AWS credentials.
- In `~/.aws/config`, only when you set up access in the app and confirm the exact lines shown.
- SSO sign-ins use the AWS CLI's token cache (`~/.aws/sso/cache`), so the CLI and the app share a sign-in.

IAM tokens for the database live only in memory and expire after 15 minutes.

## DSQL limitations

DSQL is PostgreSQL-compatible but not PostgreSQL: no foreign keys, no `TRUNCATE`, no sequences or `SERIAL`, one DDL
statement per transaction, at most 3,000 rows and about 10 MiB changed per transaction, optimistic concurrency
(conflicts surface as `40001` at commit), and others. The app warns about these before running a statement. For the
full list and how a Laravel app copes with them, see the README of
[`multiapi/laravel-aurora-dsql`](https://github.com/multi-api/laravel-aurora-dsql).

## Troubleshooting

| You see | What to do |
| --- | --- |
| "Your AWS sign-in has expired" | Click **Sign in again** (or run `aws sso login --profile <name>`). |
| No clusters listed | Add the cluster's region to the search, check `dsql:ListClusters`, or **enter an endpoint**. |
| `permission denied for table …` | The table's owner has not granted your role. The explanation shows the `GRANT`; **Access… → Check my access** lists every missing one. |
| `relation … does not exist` | You are in another schema: pick it in the top bar. |
| `change conflicts with another transaction` (`40001`) | Another transaction changed the same rows. The app retries its own edits; rerun your statement. |
| macOS says the app cannot be opened | It is unsigned: **System Settings → Privacy & Security → Open Anyway**. |

## For whoever builds this

Read, in order:

1. [`CLAUDE.md`](CLAUDE.md) — working rules (language, AWS safety, commits, what never goes in this public repo).
2. [Design spec](docs/superpowers/specs/2026-11-04-arco-dsql-studio-design.md) — what to build and why.
3. [Implementation plan](docs/superpowers/plans/2026-11-04-arco-dsql-studio-mvp.md) — 18 tasks.
4. [`docs/development.md`](docs/development.md) — toolchain, commands, tests.

## Licence

MIT — see [LICENSE](LICENSE).
