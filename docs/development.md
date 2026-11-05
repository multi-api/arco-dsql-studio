# Development

How to build, run and test Arco DSQL Studio from source.

## Layout

- `crates/dsql-core/` — plain Rust library with all AWS, token, Postgres and SQL logic. No Tauri dependency, so it is
  tested with `cargo test` alone.
- `src-tauri/` — the Tauri 2 app: a thin layer of commands over `dsql-core` (`src/commands/`), and `AppState`
  (`src/state.rs`) with pending SSO sign-ins and open connection pools. SSO tokens and credentials stay there.
- `src/api.ts` — a typed wrapper per Tauri command; `src/types.ts` mirrors the Rust types (snake_case fields,
  externally tagged enums such as `{ "Db": { ... } }`). Change both when a command changes.
- `src/` — the React + TypeScript UI (Vite). It calls Tauri commands and never sees AWS credentials or tokens.
- The repository root is a Cargo workspace (`crates/dsql-core`, `src-tauri`), so `target/` lives at the root.

## Toolchain

| Tool | Version | Check |
| --- | --- | --- |
| Rust | stable, 1.99 or later (via rustup) | `cargo --version` |
| Node.js | 22 or later (26 also works) | `node --version` |
| Tauri prerequisites | macOS: Xcode or its command line tools. Windows: Microsoft C++ Build Tools and WebView2. | `xcode-select -p` on macOS |

Install Rust with rustup: `brew install rustup && rustup default stable` on macOS, `winget install Rustlang.Rustup`
on Windows, or the installer from <https://rustup.rs>. If `rustup` is installed but `cargo` is not on `PATH`, add
`~/.cargo/bin` (or the toolchain's `bin` directory under `~/.rustup/toolchains/`).

The AWS CLI v2 is handy for checks but the app does not need it.

## Commands

```sh
npm install                 # once, installs the UI and the Tauri CLI
npm run tauri dev           # run the app with hot reload; opens the "Arco DSQL Studio" window
npm test                    # UI tests (Vitest + Testing Library, jsdom)
cargo test -p dsql-core     # Rust core unit tests
cargo test --workspace      # every Rust test, including the Tauri app crate
cargo fmt --all             # format Rust code (CI-style check: cargo fmt --all -- --check)
cargo clippy --workspace --all-targets   # lints; keep it warning-free
npm run build               # type-check and build the UI into dist/
npm run tauri build         # release bundle (.app/.dmg on macOS)
```

## Release builds

`npm run tauri build` builds the release app and its bundles under `target/release/bundle/` (`bundle.targets` is
`all`, so each platform makes its own: `.app` and `.dmg` on macOS, `.msi` and NSIS `.exe` on Windows). On an Apple
silicon Mac, `npm run tauri build -- --target aarch64-apple-darwin` makes the arm64 build (the output then goes under
`target/aarch64-apple-darwin/release/bundle/`); macOS 11 is the minimum (`bundle.macOS.minimumSystemVersion`).

The builds are unsigned. On macOS, users open them once through **System Settings → Privacy & Security → Open Anyway**;
signing and notarisation need the owner's Apple Developer ID and are not set up yet. Windows SmartScreen may warn the
same way.

`npm run tauri build -- --no-bundle` compiles the release binary only (`target/release/arco-dsql-studio`), without
the installer tools, which Tauri downloads the first time it bundles.

## Integration tests

Integration tests run against a real Aurora DSQL cluster and are skipped when these environment variables are unset:

| Variable | Meaning |
| --- | --- |
| `DSQL_HOST` | Cluster endpoint, `<id>.dsql.<region>.on.aws` |
| `DSQL_REGION` | Cluster region |
| `AWS_PROFILE` | AWS profile used to sign in |
| `DSQL_TEST_SCHEMA` | Schema holding the test tables |
| `DSQL_CONSOLE_FUNCTION` | Arco console Lambda for `it_console_tinker`, which runs a harmless `echo` inside a real app: set it only with the owner's OK |

On a scratch cluster, `cargo run -p dsql-core --example spike -- setup` (see below) creates the test schema
`spike_app`: tables owned by a role `spike_owner`, which `admin` cannot read, as in an Arco app. Use
`DSQL_TEST_SCHEMA=spike_app`. `it_role_lifecycle` runs the owner grants there through `admin`'s temporary membership in
`spike_owner` (no console Lambda needed) and removes the role it creates.

The live tests take a shared lock and run one at a time: a test that changes the schema makes another test's catalog
read fail with `OC001` ("schema has been updated by another transaction"). `it_apply_update_roundtrip` drops any
`public.arco_it_edits_*` table an interrupted run left behind.

This repository is public: never commit real values (account IDs, cluster IDs, ARNs, profile names, SSO start URLs).
Keep them in your shell or in a `.env` file, which is git-ignored.

Run the integration tests with the variables set, for example:

```sh
AWS_PROFILE=... DSQL_REGION=... cargo test -p dsql-core --test integration -- --nocapture
```

`src-tauri/tests/ipc.rs` calls the commands over IPC with Tauri's mock runtime, with the JSON the UI sends. Its live
test (`ipc_live_connection`) uses the same variables:

```sh
AWS_PROFILE=... DSQL_REGION=... DSQL_HOST=... DSQL_TEST_SCHEMA=... cargo test -p arco-dsql-studio --test ipc
```

On Windows, `src-tauri/build.rs` embeds the Common Controls v6 manifest (`windows-app-manifest.xml`) in every binary,
not just the app; without it test binaries that link Tauri exit with `STATUS_ENTRYPOINT_NOT_FOUND`.

## Local data

The app keeps saved connections, query history and settings in `arco-dsql-studio.sqlite` in its data directory
(`~/Library/Application Support/cl.multiapi.arco-dsql-studio/` on macOS, `%APPDATA%\cl.multiapi.arco-dsql-studio\` on
Windows). It never stores passwords, tokens or AWS credentials; delete the file to start over.

## The DSQL spike

`crates/dsql-core/examples/spike.rs` reproduces the checks in [`spike-findings.md`](spike-findings.md). Run it only
against a scratch cluster, never an app's cluster: it creates and drops roles, an IAM mapping, a schema and tables.

```sh
export DSQL_HOST=... DSQL_REGION=... AWS_PROFILE=...
export SPIKE_IAM_ROLE_ARN=...   # the IAM role behind AWS_PROFILE (with its path), for the spike roles' mappings
cargo run -p dsql-core --example spike -- setup     # owner role, schema spike_app, tables with sample rows
cargo run -p dsql-core --example spike -- run       # every check; prints each statement's outcome
cargo run -p dsql-core --example spike -- teardown  # removes everything setup and run created
```

## Checking SSO sign-in by hand

`crates/dsql-core/examples/sso_login.rs` runs the device flow the app uses: it prints a URL and a code, waits for you
to approve in the browser, lists the accounts and roles you can reach, and writes the token to the directory you pass.
Pass a temporary directory to leave the AWS CLI's own `~/.aws/sso/cache` alone, and delete it afterwards (the token is
a secret).

```sh
cargo run -p dsql-core --example sso_login -- <start_url> <sso_region> <session_name> <temp_dir>
```
