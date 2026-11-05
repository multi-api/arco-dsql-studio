# Working on Arco DSQL Studio

Instructions for AI agents (and people) building this repository.

## Where things are

- What and why: `docs/superpowers/specs/2026-11-04-arco-dsql-studio-design.md`.
- How, task by task: `docs/superpowers/plans/2026-11-04-arco-dsql-studio-mvp.md`. Follow it in order; tick the
  checkboxes as you go and commit the plan with each task. If a spike result changes a decision, update the plan and
  the spec in the same commit and say why.
- Reference for DSQL roles and schemas: the "Querying the database yourself" section of Arco Cloud's
  `docs/databases.md`, summarised in spec section 6.5. DSQL's unsupported features: the README of
  [`multiapi/laravel-aurora-dsql`](https://github.com/multi-api/laravel-aurora-dsql).

## Status

- 2026-11-04: spec, plan, README, licence.
- 2026-11-04: Task 0 done (Tauri 2 scaffold, `dsql-core` with `CoreError`, Vitest; see `docs/development.md`).
- 2026-11-04: Task 1 done on a scratch cluster the owner approved (not an Arco app); see `docs/spike-findings.md`.
  Next: Task 2 (AWS profiles).

## Rules

- **Language:** talk to the owner in Spanish. Code, comments, docs and commit messages in English.
- **Public repository (MIT).** Never commit AWS account IDs, cluster IDs, IAM/STS ARNs, profile names, SSO start URLs
  or anything from a real account. Integration tests read `DSQL_HOST`, `DSQL_REGION`, `AWS_PROFILE`,
  `DSQL_TEST_SCHEMA`, `DSQL_CONSOLE_FUNCTION` and are skipped when unset. The owner keeps the real test values
  outside the repository; ask the owner for them.
- **AWS safety:**
  - Before any AWS call, confirm the profile and account with the owner.
  - Never create credentials, IAM users, roles, policies or access keys.
  - Destructive AWS actions are the owner's.
  - SQL experiments on a real cluster clean up after themselves (drop test roles, mappings and tables).
  - Running SQL as the tables' owner (through the app's console Lambda) needs the owner's OK each time.
- **The app itself** never stores secrets, only appends to `~/.aws/config` after showing the exact lines, and always
  previews and confirms production edits.
- **Toolchain (this may be a fresh machine):** check `cargo --version`, `node --version` (22), `xcode-select -p`. If
  Rust is missing, install it with rustup (`brew install rustup && rustup default stable`, or the installer from
  rustup.rs) — ask the owner before installing anything. If `rustup` exists but `cargo` is not on `PATH`, add
  `~/.rustup/toolchains/stable-aarch64-apple-darwin/bin`. Tauri's macOS prerequisites: Xcode or its command line
  tools. On Windows: `winget install Rustlang.Rustup`, plus the Microsoft C++ Build Tools and WebView2; `cargo` lives
  in `~/.cargo/bin`. The AWS CLI v2 is handy for checks but the app does not need it.
- **AWS test values:** the owner gives them to you (test account, SSO profile, cluster endpoint, test schema, console
  function). On a new machine the owner also sets up the SSO profile; never write those values into the repository.
- **Tauri 2.x**, not the 3.0 alpha.
- **Tests first** (TDD) for the Rust core and the UI behaviours listed in the plan.
- **Document everything:** every user-visible behaviour goes in `README.md` or `docs/*.md` in the same task that
  builds it.
- **Commits** end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. **PR bodies** end with
  `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.
- **Publishing:** ask the owner for an explicit "si" before pushing, opening PRs or releasing.
- Do not change other repositories (such as Arco Cloud) from here. Write cross-repository requests as notes for the
  owner.
