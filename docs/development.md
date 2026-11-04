# Development

How to build, run and test Arco DSQL Studio from source.

## Layout

- `crates/dsql-core/` — plain Rust library with all AWS, token, Postgres and SQL logic. No Tauri dependency, so it is
  tested with `cargo test` alone.
- `src-tauri/` — the Tauri 2 app: a thin layer of commands over `dsql-core`.
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
npm run build               # type-check and build the UI into dist/
npm run tauri build         # release bundle (.app/.dmg on macOS)
```

## Integration tests

Integration tests run against a real Aurora DSQL cluster and are skipped when these environment variables are unset:

| Variable | Meaning |
| --- | --- |
| `DSQL_HOST` | Cluster endpoint, `<id>.dsql.<region>.on.aws` |
| `DSQL_REGION` | Cluster region |
| `AWS_PROFILE` | AWS profile used to sign in |
| `DSQL_TEST_SCHEMA` | Schema holding the test tables |
| `DSQL_CONSOLE_FUNCTION` | Arco console Lambda, for the role wizard test |

This repository is public: never commit real values (account IDs, cluster IDs, ARNs, profile names, SSO start URLs).
Keep them in your shell or in a `.env` file, which is git-ignored.
