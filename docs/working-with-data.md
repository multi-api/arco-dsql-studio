# Working with data

What you see after connecting: the schema tree, SQL tabs, results and history.

## The top bar

The connection's name, the role you are signed in as, and the **schema** your queries run in (`search_path`). Pick
another schema there to switch; new queries run in it at once, so `select * from users` finds the right `users`.

- A red **PRODUCTION** badge (and a red line under the bar) when the connection is marked as production.
- **read-only** when the connection only runs queries that read.
- **History** opens the history panel; **Disconnect** goes back to your connections.

Switch schema with the picker, not with `SET search_path` in the editor: each run may use a different pooled
connection, so a `SET` only lasts for the run it is in.

## Schema tree

Schemas → tables, views and sequences → columns (type, `NOT NULL`, default, `PK`) and indexes. Branches load when you
open them.

- 🔒 marks a table your role cannot read. Hover a table to see its owner and what your role can do with it (`SELECT`,
  `INSERT`, `UPDATE`, `DELETE`).
- Double-click a table, or **Query this table**, to open a tab with `SELECT * FROM "schema"."table" LIMIT 100;`.

## SQL tabs

Each tab is a SQL editor with PostgreSQL highlighting and autocomplete for the current schema's tables and columns.
**+** opens a new tab; **×** closes one.

| Action | Shortcut | Runs |
| --- | --- | --- |
| **Run** | ⌘↵ (macOS), Ctrl+↵ (Windows, Linux) | the selection, or the statement under the cursor |
| **Run all** | ⇧⌘↵, Shift+Ctrl+↵ | the whole tab, one statement at a time |
| **Cancel** | — | stops the running query |

Statements run one at a time and stop at the first error, because DSQL allows a single DDL statement per transaction.
A transaction must begin and end in the same run (`BEGIN; …; COMMIT;` together): after a run that contains `BEGIN`,
`COMMIT` or `ROLLBACK`, its connection is closed, which rolls back anything left open.

**Cancel** closes the query's connection, because DSQL ignores Postgres cancel requests. The query may still finish
on the server; if it was changing data, check the result.

## Before a query runs

The app reads the SQL first:

- **DSQL will probably reject it**, for example `TRUNCATE`, `CREATE INDEX` without `ASYNC`, adding a constraint to an
  existing table, `SAVEPOINT`, temporary tables, triggers, PL/pgSQL, extensions, advisory locks, `FOR SHARE`, a text
  search configuration other than `simple`, `CREATE DATABASE`, or more than one DDL statement (or DDL next to data
  changes) in one transaction. A dialog explains why and what to do instead in DSQL, with a link to the AWS docs.
  **Run anyway** still sends it.
- **Production**: anything that writes data or changes the schema asks first, in a red dialog listing those
  statements. Reads run straight away.
- **Read-only**: anything that writes data or changes the schema is not run, and the app says why.

## Results

The grid shows the last result of the run with its row count and duration; messages such as `INSERT · 3 rows` appear
above it. `NULL` is a dimmed badge; an empty string is an empty cell; the text `NULL` is plain text. Up to 10,000 rows
are kept; if a query returns more, the grid says so (add `LIMIT` or a `WHERE`).

Errors are explained in plain language with the fix when there is one: switch to the schema that has the table, the
`GRANT` the table's owner must run (with **Copy**), sign in again, or **Try again** after a concurrency conflict
(`40001`). The app never re-runs your SQL by itself.

## History

Every run is saved with its connection, time, duration, rows and whether it failed. The panel searches it (newest
first); click an entry to open it in a new tab. **Clear history** deletes all of it after a confirmation, and
**Keep history** turns saving off (and on again).

The history lives only on this computer, in the app's local database (see [development.md](development.md#local-data)
for where). It keeps your SQL text, never results or passwords.
