import { useCallback, useEffect, useRef, useState } from "react";

import {
  cancel,
  checkSql,
  explainError,
  listSchemas,
  listTables,
  runSql,
  saveConnection,
  setSchema,
  statementAt,
  tableColumns,
} from "../api";
import ExplainBox from "../components/ExplainBox";
import HistoryPanel from "../components/HistoryPanel";
import ResultsGrid from "../components/ResultsGrid";
import SchemaTree from "../components/SchemaTree";
import SqlEditor, { type Selection } from "../components/SqlEditor";
import CheckAccess from "../components/CheckAccess";
import RoleWizard from "../components/RoleWizard";
import TableBrowser from "../components/TableBrowser";
import TopBar from "../components/TopBar";
import { errorText } from "../errors";
import type {
  ConnectionHandle,
  CoreError,
  Explanation,
  Finding,
  SavedConnection,
  SchemaInfo,
  ScriptResult,
  SqlCheck,
} from "../types";

interface Tab {
  id: number;
  title: string;
  sql: string;
  selection: Selection;
  running: boolean;
  result: ScriptResult | null;
  problem: Explanation | null;
  /** SQL whose explained error can be retried with one click (an OCC conflict). */
  retry: string | null;
  /** Shown instead of running: read-only refusal. */
  notice: string | null;
  /** Set for a table tab (the spreadsheet view) instead of a SQL editor. */
  table: { schema: string; name: string } | null;
  /** Set for an access tool tab: the team access wizard, role removal, or the access check. */
  tool: Tool | null;
}

type Tool = "team-access" | "remove-role" | "check-access";

const TOOL_TITLES: Record<Tool, string> = {
  "team-access": "Team access",
  "remove-role": "Remove a role",
  "check-access": "Check access",
};

/** Waiting for the user: DSQL warnings, then the production confirmation. */
interface Pending {
  tabId: number;
  sql: string;
  check: SqlCheck;
  stage: "warn" | "confirm";
}

let nextTabId = 1;

function newTab(sql = "", title?: string, table: Tab["table"] = null): Tab {
  const id = nextTabId++;
  return {
    id,
    title: title ?? (sql.trim() ? sql.trim().split("\n")[0].slice(0, 32) : `Query ${id}`),
    sql,
    selection: { cursor: sql.length, selected: "" },
    running: false,
    result: null,
    problem: null,
    retry: null,
    notice: null,
    table,
    tool: null,
  };
}

const changesSchema = (check: SqlCheck) => check.classified.statements.some((s) => s.kind === "Schema");

const q = (name: string) => `"${name.replace(/"/g, '""')}"`;

export default function Workspace({
  handle,
  saved,
  onDisconnect,
}: {
  handle: ConnectionHandle;
  saved: SavedConnection;
  onDisconnect: () => void;
}) {
  const [schemas, setSchemas] = useState<SchemaInfo[]>([]);
  const [schema, setCurrentSchema] = useState<string | null>(handle.schema);
  const [tabs, setTabs] = useState<Tab[]>(() => [newTab()]);
  const [active, setActive] = useState(() => tabs[0].id);
  const [pending, setPending] = useState<Pending | null>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [historyVersion, setHistoryVersion] = useState(0);
  const [completions, setCompletions] = useState<Record<string, string[]>>({});
  const [treeVersion, setTreeVersion] = useState(0);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;

  const tab = tabs.find((t) => t.id === active) ?? tabs[0];
  const update = useCallback((id: number, patch: Partial<Tab>) => {
    setTabs((ts) => ts.map((t) => (t.id === id ? { ...t, ...patch } : t)));
  }, []);

  useEffect(() => {
    listSchemas(handle.id)
      .then(setSchemas)
      .catch(() => setSchemas([]));
  }, [handle.id]);

  // Autocomplete: the current schema's tables, then their columns as they arrive.
  useEffect(() => {
    let live = true;
    setCompletions({});
    if (!schema) return;
    (async () => {
      try {
        const tables = await listTables(handle.id, schema);
        if (!live) return;
        setCompletions(Object.fromEntries(tables.map((t) => [t.name, []])));
        for (const t of tables) {
          const cols = await tableColumns(handle.id, schema, t.name).catch(() => []);
          if (!live) return;
          setCompletions((c) => ({ ...c, [t.name]: cols.map((col) => col.name) }));
        }
      } catch {
        // Autocomplete is a convenience; the tree shows catalog errors.
      }
    })();
    return () => {
      live = false;
    };
  }, [handle.id, schema]);

  function openTab(sql: string, title?: string) {
    const t = newTab(sql, title);
    setTabs((ts) => [...ts, t]);
    setActive(t.id);
  }

  /** Access tools open once per schema; opening one again goes back to its tab. */
  function openTool(tool: Tool) {
    const existing = tabsRef.current.find((t) => t.tool === tool);
    if (existing) {
      setActive(existing.id);
      return;
    }
    const t = { ...newTab("", TOOL_TITLES[tool]), tool };
    setTabs((ts) => [...ts, t]);
    setActive(t.id);
  }

  /** A table opens once; opening it again goes back to its tab. */
  function openTableTab(schemaName: string, name: string) {
    const existing = tabsRef.current.find((t) => t.table?.schema === schemaName && t.table.name === name);
    if (existing) {
      setActive(existing.id);
      return;
    }
    const t = newTab("", name, { schema: schemaName, name });
    setTabs((ts) => [...ts, t]);
    setActive(t.id);
  }

  function closeTab(id: number) {
    setTabs((ts) => {
      const rest = ts.filter((t) => t.id !== id);
      const next = rest.length ? rest : [newTab()];
      if (id === active) setActive(next[next.length - 1].id);
      return next;
    });
  }

  async function switchSchema(name: string) {
    try {
      await setSchema(handle.id, name);
      setCurrentSchema(name);
      setTreeVersion((v) => v + 1);
    } catch (err) {
      update(tab.id, { problem: { title: "Could not switch schema", body: errorText(err), fix: "None" } });
    }
  }

  async function run(all: boolean) {
    const t = tab;
    const sql = all ? t.sql : t.selection.selected.trim() ? t.selection.selected : await statementAt(t.sql, t.selection.cursor);
    if (!sql?.trim()) return;
    update(t.id, { notice: null, problem: null });
    let check: SqlCheck;
    try {
      check = await checkSql(handle.id, sql);
    } catch (err) {
      update(t.id, { problem: { title: "Could not check the SQL", body: errorText(err), fix: "None" } });
      return;
    }
    if (check.blocked) {
      update(t.id, {
        notice:
          "This connection is read-only, so statements that write data or change the schema are not run. " +
          "Open the connection without read-only to run them.",
      });
      return;
    }
    const findings = check.classified.statements.flatMap((s) => s.findings);
    if (findings.length) setPending({ tabId: t.id, sql, check, stage: "warn" });
    else if (check.needs_confirmation) setPending({ tabId: t.id, sql, check, stage: "confirm" });
    else execute(t.id, sql, changesSchema(check));
  }

  function proceed() {
    if (!pending) return;
    if (pending.stage === "warn" && pending.check.needs_confirmation) {
      setPending({ ...pending, stage: "confirm" });
      return;
    }
    setPending(null);
    execute(pending.tabId, pending.sql, changesSchema(pending.check));
  }

  async function execute(tabId: number, sql: string, schemaChanged = false) {
    update(tabId, { running: true, problem: null, notice: null, retry: null });
    let result: ScriptResult;
    try {
      result = await runSql(handle.id, sql);
    } catch (err) {
      update(tabId, { running: false, problem: await explain(err as CoreError), retry: null });
      return;
    }
    setHistoryVersion((v) => v + 1);
    if (schemaChanged) {
      // New or dropped tables and schemas show up in the tree and the schema picker.
      listSchemas(handle.id)
        .then(setSchemas)
        .catch(() => {});
      setTreeVersion((v) => v + 1);
    }
    const failed = result.outcomes.find((o) => "Error" in o);
    const err = failed && "Error" in failed ? failed.Error : null;
    const occ = !!err && typeof err === "object" && "Db" in err && err.Db.sqlstate === "40001";
    update(tabId, {
      running: false,
      result,
      problem: err ? await explain(err) : null,
      // An OCC conflict is offered as a one-click retry, never replayed automatically.
      retry: occ ? sql : null,
    });
  }

  async function explain(err: CoreError): Promise<Explanation> {
    if (err === "Cancelled") return { title: "Cancelled", body: "The query was stopped.", fix: "None" };
    try {
      return await explainError(handle.id, err);
    } catch {
      return { title: "The query failed", body: errorText(err), fix: "None" };
    }
  }

  const findings: Finding[] = pending?.check.classified.statements.flatMap((s) => s.findings) ?? [];
  const writes = pending?.check.classified.statements.filter((s) => s.kind === "Write" || s.kind === "Schema") ?? [];
  const rowSets = tab.result?.outcomes.flatMap((o) => ("Rows" in o ? [o.Rows] : [])) ?? [];
  const messages = tab.result?.outcomes.flatMap((o) => ("Command" in o ? [o.Command] : [])) ?? [];

  return (
    <div className="workspace">
      <TopBar
        saved={saved}
        user={handle.current_user}
        schema={schema}
        schemas={schemas}
        onSchema={switchSchema}
        historyOpen={historyOpen}
        onToggleHistory={() => setHistoryOpen((o) => !o)}
        isAdmin={handle.current_user === "admin"}
        onTool={openTool}
        onDisconnect={onDisconnect}
      />
      <div className={`workspace-body${historyOpen ? " with-history" : ""}`}>
        <div className="side">
          <div className="side-head">
            <h3>Schemas</h3>
            <button
              type="button"
              className="link subtle"
              aria-label="Reload schemas and tables"
              title="Reload schemas and tables"
              onClick={() => {
                listSchemas(handle.id)
                  .then(setSchemas)
                  .catch(() => {});
                setTreeVersion((v) => v + 1);
              }}
            >
              ↻
            </button>
          </div>
        <SchemaTree
          key={treeVersion}
          connectionId={handle.id}
          schemas={schemas}
          current={schema}
          onOpenTable={openTableTab}
          onQueryTable={(s, t) => openTab(`SELECT * FROM ${q(s)}.${q(t)} LIMIT 100;`, t)}
        />
        </div>

        <section className={`center${tab.table || tab.tool ? " table-mode" : ""}`}>
          <div className="tabs" role="tablist">
            {tabs.map((t) => (
              <div key={t.id} className={`tab${t.id === tab.id ? " active" : ""}`}>
                <button type="button" role="tab" aria-selected={t.id === tab.id} onClick={() => setActive(t.id)}>
                  {t.title}
                  {t.running && " …"}
                </button>
                <button type="button" className="close" aria-label={`Close ${t.title}`} onClick={() => closeTab(t.id)}>
                  ×
                </button>
              </div>
            ))}
            <button type="button" className="ghost new-tab" aria-label="New query tab" onClick={() => openTab("")}>
              +
            </button>
          </div>

          {tab.tool ? (
            !schema ? (
              <p className="tool">Pick a schema first.</p>
            ) : tab.tool === "check-access" ? (
              <CheckAccess key={`${tab.id}-${schema}`} connectionId={handle.id} schema={schema} role={handle.current_user} />
            ) : (
              <RoleWizard
                key={`${tab.id}-${schema}`}
                mode={tab.tool === "team-access" ? "create" : "remove"}
                connectionId={handle.id}
                saved={saved}
                schema={schema}
                onSaveConnection={(c) => saveConnection(c).catch(() => {})}
              />
            )
          ) : tab.table ? (
            <TableBrowser
              key={tab.id}
              connectionId={handle.id}
              schema={tab.table.schema}
              table={tab.table.name}
              production={saved.production}
              readOnly={saved.read_only}
              onOpenSql={(sql) => openTab(sql)}
            />
          ) : (
            <>
          <div className="toolbar">
            <button type="button" onClick={() => run(false)} disabled={tab.running} title="⌘↵ / Ctrl+↵">
              Run
            </button>
            <button type="button" className="ghost" onClick={() => run(true)} disabled={tab.running} title="⇧⌘↵ / Shift+Ctrl+↵">
              Run all
            </button>
            {tab.running && (
              <button type="button" className="ghost" onClick={() => cancel(handle.id)}>
                Cancel
              </button>
            )}
            <span className="muted hint">Run: statement at the cursor, or the selection.</span>
          </div>

          <SqlEditor
            key={tab.id}
            value={tab.sql}
            onChange={(sql) => update(tab.id, { sql })}
            onSelection={(selection) => update(tab.id, { selection })}
            onRun={() => run(false)}
            onRunAll={() => run(true)}
            completions={completions}
            defaultSchema={schema}
          />

          <div className="output">
            {tab.running && <p className="muted">Running…</p>}
            {tab.notice && <p className="notice">{tab.notice}</p>}
            {tab.problem && (
              <ExplainBox
                explanation={tab.problem}
                connectionId={handle.id}
                onSchemaChanged={(s) => {
                  setCurrentSchema(s);
                  setTreeVersion((v) => v + 1);
                }}
                onRetry={tab.retry ? () => execute(tab.id, tab.retry as string) : undefined}
              />
            )}
            {messages.map((c, i) => (
              <p key={i} className="muted">
                {c.tag}
                {c.rows_affected ? ` · ${c.rows_affected} rows` : ""}
              </p>
            ))}
            {rowSets.length > 0 && (
              <ResultsGrid result={rowSets[rowSets.length - 1]} durationMs={tab.result?.duration_ms ?? 0} />
            )}
          </div>
            </>
          )}
        </section>

        {historyOpen && <HistoryPanel refresh={historyVersion} onOpen={(sql) => openTab(sql)} />}
      </div>

      {pending && (
        <div className="modal-backdrop">
          <div
            className={`modal${pending.stage === "confirm" ? " danger" : ""}`}
            role="dialog"
            aria-modal="true"
            aria-labelledby="pending-title"
          >
            {pending.stage === "warn" ? (
              <>
                <h2 id="pending-title">DSQL will probably reject this</h2>
                {findings.map((f, i) => (
                  <div key={i} className="finding">
                    <p>
                      <strong>{f.message}</strong>
                    </p>
                    <p>{f.alternative}</p>
                    {f.docs_url && (
                      <p className="muted">
                        <a href={f.docs_url} target="_blank" rel="noreferrer">
                          AWS documentation
                        </a>
                      </p>
                    )}
                  </div>
                ))}
                <div className="actions">
                  <button type="button" className="ghost" onClick={() => setPending(null)}>
                    Cancel
                  </button>
                  <button type="button" onClick={proceed}>
                    Run anyway
                  </button>
                </div>
              </>
            ) : (
              <>
                <h2 id="pending-title">Run on production?</h2>
                <p>
                  <strong>{saved.name}</strong> is marked as production. These statements change data or the schema:
                </p>
                <pre>{writes.map((s) => s.sql).join(";\n")}</pre>
                <div className="actions">
                  <button type="button" className="ghost" onClick={() => setPending(null)} autoFocus>
                    Cancel
                  </button>
                  <button type="button" className="danger" onClick={proceed}>
                    Run on production
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
