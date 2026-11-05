import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { useCallback, useEffect, useMemo, useState } from "react";

import { applyChanges, explainError, previewChanges, tableColumns, tablePage } from "../api";
import { toCsv, toInserts, toJson, toTsv } from "../copy";
import { errorText } from "../errors";
import type {
  ChangeSet,
  ColumnInfo,
  CoreError,
  Explanation,
  Filter,
  FilterOp,
  PageRequest,
  TablePage,
  Values,
} from "../types";
import { ResizeHandle, fitWidth, useColumnWidths } from "./ColumnResize";
import ExplainBox from "./ExplainBox";
import SaveDialog from "./SaveDialog";

export const PAGE_SIZE = 200;

const OPS: { op: FilterOp | ""; label: string }[] = [
  { op: "", label: "—" },
  { op: "Eq", label: "=" },
  { op: "Ne", label: "≠" },
  { op: "Lt", label: "<" },
  { op: "Gt", label: ">" },
  { op: "Contains", label: "contains" },
  { op: "IsNull", label: "is null" },
  { op: "IsNotNull", label: "is not null" },
];

/** Which cell is being edited: a loaded row by index, or an added row (`added`). */
type CellRef = { row: number; added: boolean; column: string };

interface Props {
  connectionId: string;
  schema: string;
  table: string;
  production: boolean;
  readOnly: boolean;
  /** Opens SQL in a new editor tab ("Show SQL" → "Open in editor"). */
  onOpenSql?: (sql: string) => void;
}

/**
 * A table as a spreadsheet: pages of 200 rows, sort, filters, a WHERE box, and staged edits that are only sent after
 * the exact statements are shown (and confirmed in production). Rows are found by primary key, so a table without one
 * is read-only.
 */
export default function TableBrowser({ connectionId, schema, table, production, readOnly, onOpenSql }: Props) {
  const [columns, setColumns] = useState<ColumnInfo[]>([]);
  const [page, setPage] = useState<PageRequest>({
    offset: 0,
    limit: PAGE_SIZE,
    order_by: [],
    filters: [],
    where_sql: null,
  });
  const [data, setData] = useState<TablePage | null>(null);
  const [loading, setLoading] = useState(false);
  const [problem, setProblem] = useState<Explanation | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const [filterDraft, setFilterDraft] = useState<Record<string, { op: FilterOp | ""; value: string }>>({});
  const [showFilters, setShowFilters] = useState(false);
  const [whereDraft, setWhereDraft] = useState("");
  const [showSql, setShowSql] = useState(false);

  const [edits, setEdits] = useState<Map<number, Values>>(new Map());
  const [added, setAdded] = useState<Values[]>([]);
  const [deleted, setDeleted] = useState<Set<number>>(new Set());
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [editing, setEditing] = useState<CellRef | null>(null);
  const [draft, setDraft] = useState("");

  const [preview, setPreview] = useState<string[][] | null>(null);
  const [saving, setSaving] = useState(false);
  const [conflict, setConflict] = useState(false);

  const pk = useMemo(
    () => columns.filter((c) => c.is_pk).sort((a, b) => a.ordinal - b.ordinal).map((c) => c.name),
    [columns],
  );
  const editable = !readOnly && pk.length > 0;
  const changeCount = [...edits.keys()].filter((i) => !deleted.has(i)).length + deleted.size + added.length;

  const explain = useCallback(
    async (err: unknown): Promise<Explanation> => {
      try {
        return await explainError(connectionId, err as CoreError);
      } catch {
        return { title: "Something went wrong", body: errorText(err), fix: "None" };
      }
    },
    [connectionId],
  );

  useEffect(() => {
    tableColumns(connectionId, schema, table)
      .then(setColumns)
      .catch(async (err) => setProblem(await explain(err)));
  }, [connectionId, schema, table, explain]);

  const load = useCallback(async () => {
    setLoading(true);
    setProblem(null);
    try {
      setData(await tablePage(connectionId, schema, table, page));
    } catch (err) {
      setData(null);
      setProblem(await explain(err));
    } finally {
      setLoading(false);
    }
  }, [connectionId, schema, table, page, explain]);

  useEffect(() => {
    load();
  }, [load]);

  function discard() {
    setEdits(new Map());
    setAdded([]);
    setDeleted(new Set());
    setSelected(new Set());
    setEditing(null);
    setConflict(false);
  }

  function changePage(patch: Partial<PageRequest>) {
    if (changeCount > 0 && !window.confirm("Discard the changes you have not saved?")) return;
    discard();
    setPage((p) => ({ ...p, ...patch }));
  }

  // Sorting and filtering

  function sortBy(column: string) {
    const current = page.order_by[0];
    const next: [string, boolean][] =
      current?.[0] !== column ? [[column, true]] : current[1] ? [[column, false]] : [];
    changePage({ order_by: next, offset: 0 });
  }

  function applyFilters() {
    const filters: Filter[] = Object.entries(filterDraft)
      .filter(([, f]) => f.op !== "")
      .map(([column, f]) => ({
        column,
        op: f.op as FilterOp,
        value: f.op === "IsNull" || f.op === "IsNotNull" ? null : f.value,
      }));
    changePage({ filters, where_sql: whereDraft.trim() || null, offset: 0 });
  }

  // Editing

  const resultColumns = useMemo(() => data?.result.columns ?? [], [data]);
  const rows = data?.result.rows ?? [];
  const valueOf = (row: number, column: string): string | null => {
    const staged = edits.get(row);
    if (staged && column in staged) return staged[column];
    return rows[row]?.[resultColumns.indexOf(column)] ?? null;
  };

  function startEdit(ref: CellRef) {
    if (!editable || (!ref.added && deleted.has(ref.row))) return;
    setEditing(ref);
    const current = ref.added ? (added[ref.row][ref.column] ?? null) : valueOf(ref.row, ref.column);
    setDraft(current ?? "");
  }

  function stage(value: string | null) {
    if (!editing) return;
    const { row, column } = editing;
    if (editing.added) {
      setAdded((a) => a.map((r, i) => (i === row ? { ...r, [column]: value } : r)));
    } else {
      const original = rows[row]?.[resultColumns.indexOf(column)] ?? null;
      setEdits((e) => {
        const next = new Map(e);
        const set = { ...(next.get(row) ?? {}) };
        if (value === original) delete set[column];
        else set[column] = value;
        if (Object.keys(set).length) next.set(row, set);
        else next.delete(row);
        return next;
      });
    }
    setEditing(null);
  }

  function toggleSelected(row: number) {
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(row)) next.delete(row);
      else next.add(row);
      return next;
    });
  }

  function deleteSelected() {
    setDeleted((d) => new Set([...d, ...selected]));
    setSelected(new Set());
  }

  function changeSet(): ChangeSet {
    const pkValues = (row: number): Values => Object.fromEntries(pk.map((c) => [c, rows[row][resultColumns.indexOf(c)]]));
    return {
      schema,
      table,
      pk,
      column_types: Object.fromEntries(columns.map((c) => [c.name, c.data_type])),
      updates: [...edits.entries()]
        .filter(([row]) => !deleted.has(row))
        .sort(([a], [b]) => a - b)
        .map(([row, set]) => ({ pk_values: pkValues(row), set })),
      inserts: added,
      deletes: [...deleted].sort((a, b) => a - b).map(pkValues),
    };
  }

  async function startSave() {
    setNotice(null);
    setProblem(null);
    try {
      setPreview(await previewChanges(changeSet()));
    } catch (err) {
      setProblem(await explain(err));
    }
  }

  async function apply() {
    setSaving(true);
    try {
      const report = await applyChanges(connectionId, changeSet());
      setPreview(null);
      const retried = report.retries ? ` after ${report.retries} ${report.retries === 1 ? "retry" : "retries"}` : "";
      const missing = report.rows_missing
        ? ` ${report.rows_missing} ${report.rows_missing === 1 ? "row was" : "rows were"} no longer there.`
        : "";
      if (report.error) {
        const err = report.error;
        const occ = typeof err === "object" && "Db" in err && err.Db.sqlstate === "40001";
        setConflict(occ);
        setProblem(
          occ
            ? {
                title: "Someone else changed the same rows",
                body: `The save was retried${retried} and still conflicted. Reload to see their changes, then edit again.`,
                fix: "None",
              }
            : await explain(err),
        );
        if (report.batches_applied > 0) {
          setNotice(`${report.batches_applied} of ${report.batches_total} transactions were saved before the error.`);
        }
      } else {
        setNotice(`Saved: ${report.rows_affected} ${report.rows_affected === 1 ? "row" : "rows"} changed${retried}.${missing}`);
        discard();
        load();
      }
    } catch (err) {
      setPreview(null);
      setProblem(await explain(err));
    } finally {
      setSaving(false);
    }
  }

  // Copy and export

  function rowsForCopy(): (string | null)[][] {
    const picked = selected.size ? [...selected].sort((a, b) => a - b) : rows.map((_, i) => i);
    return picked.map((i) => resultColumns.map((c) => valueOf(i, c)));
  }

  async function copy(format: string) {
    const cols = resultColumns;
    const r = rowsForCopy();
    const text =
      format === "csv"
        ? toCsv(cols, r)
        : format === "tsv"
          ? toTsv(cols, r)
          : format === "json"
            ? toJson(cols, r)
            : toInserts(schema, table, cols, r);
    try {
      await navigator.clipboard.writeText(text);
      setNotice(`Copied ${r.length} ${r.length === 1 ? "row" : "rows"} as ${format.toUpperCase()}.`);
    } catch (err) {
      setNotice(`Could not copy: ${errorText(err)}`);
    }
  }

  async function exportCsv() {
    try {
      const path = await saveDialog({
        defaultPath: `${table}.csv`,
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
      if (!path) return;
      await writeTextFile(path, toCsv(resultColumns, rows.map((_, i) => resultColumns.map((c) => valueOf(i, c)))));
      setNotice(`Saved ${rows.length} rows to ${path}.`);
    } catch (err) {
      setNotice(`Could not export: ${errorText(err)}`);
    }
  }

  // Rendering

  const { widths, setWidth } = useColumnWidths(resultColumns);

  const sort = page.order_by[0];
  const cell = (ref: CellRef, value: string | null, changed: boolean) => {
    const isEditing =
      editing && editing.row === ref.row && editing.added === ref.added && editing.column === ref.column;
    if (isEditing) {
      return (
        <td key={ref.column} data-column={ref.column} className="editing">
          <div className="cell-editor">
          <input
            autoFocus
            aria-label={`Edit ${ref.column}`}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") stage(draft);
              if (e.key === "Escape") setEditing(null);
            }}
            onBlur={() => stage(draft)}
          />
          <button
            type="button"
            className="null-toggle"
            aria-label={`Set ${ref.column} to NULL`}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => stage(null)}
          >
            NULL
          </button>
          </div>
        </td>
      );
    }
    return (
      <td
        key={ref.column}
        data-column={ref.column}
        role="gridcell"
        className={changed ? "changed" : undefined}
        title={value ?? "NULL"}
        onDoubleClick={() => startEdit(ref)}
      >
        {value === null ? <span className="null">NULL</span> : value}
      </td>
    );
  };

  return (
    <div className="browser">
      <div className="toolbar">
        <strong>
          {schema}.{table}
        </strong>
        <button type="button" className="ghost" onClick={() => setShowFilters((f) => !f)} aria-pressed={showFilters}>
          Filter
        </button>
        <button type="button" className="ghost" onClick={() => setShowSql((s) => !s)} aria-pressed={showSql}>
          Show SQL
        </button>
        <button type="button" className="ghost" onClick={() => changePage({})} disabled={loading}>
          Reload
        </button>
        <select aria-label="Copy rows" value="" onChange={(e) => e.target.value && copy(e.target.value)}>
          <option value="">Copy {selected.size ? "selected" : "page"} as…</option>
          <option value="csv">CSV</option>
          <option value="tsv">TSV</option>
          <option value="json">JSON</option>
          <option value="insert">INSERT statements</option>
        </select>
        <button type="button" className="ghost" onClick={exportCsv} disabled={!rows.length}>
          Export CSV
        </button>
        <span className="spacer" />
        {editable && (
          <>
            <button type="button" className="ghost" onClick={() => setAdded((a) => [...a, {}])}>
              Add row
            </button>
            {selected.size > 0 && (
              <button type="button" className="ghost" onClick={deleteSelected}>
                Delete {selected.size} {selected.size === 1 ? "row" : "rows"}
              </button>
            )}
            {changeCount > 0 && (
              <>
                <button type="button" className="ghost" onClick={discard}>
                  Discard
                </button>
                <button type="button" onClick={startSave}>
                  Save {changeCount} {changeCount === 1 ? "change" : "changes"}
                </button>
              </>
            )}
          </>
        )}
      </div>

      {columns.length > 0 && pk.length === 0 && (
        <p className="notice" title="Edits find rows by primary key; without one, a change could hit the wrong rows.">
          This table has no primary key, so it is read-only here.
        </p>
      )}
      {readOnly && pk.length > 0 && <p className="muted">The connection is read-only, so rows cannot be edited.</p>}

      {showFilters && (
        <form
          className="where"
          onSubmit={(e) => {
            e.preventDefault();
            applyFilters();
          }}
        >
          <label>
            WHERE
            <input
              value={whereDraft}
              onChange={(e) => setWhereDraft(e.target.value)}
              placeholder="for example: created_at > now() - interval '1 day'"
            />
          </label>
          <button type="submit">Apply filters</button>
        </form>
      )}

      {showSql && data && (
        <div className="snippet">
          <pre>{data.sql}</pre>
          {onOpenSql && (
            <button type="button" className="ghost" onClick={() => onOpenSql(data.sql + ";")}>
              Open in editor
            </button>
          )}
        </div>
      )}

      {notice && <p className="muted">{notice}</p>}
      {problem && (
        <div>
          <ExplainBox explanation={problem} connectionId={connectionId} onRetry={load} />
          {conflict && (
            <button
              type="button"
              className="ghost"
              onClick={() => {
                discard();
                setProblem(null);
                load();
              }}
            >
              Reload
            </button>
          )}
        </div>
      )}

      <div className="grid-scroll browser-grid">
        <table className="grid sized" role="grid">
          <colgroup>
            <col style={{ width: 72 }} />
            {resultColumns.map((c, i) => (
              <col key={c} style={{ width: widths[i] }} />
            ))}
          </colgroup>
          <thead>
            <tr role="row">
              <th className="rownum" aria-hidden />
              {resultColumns.map((c) => (
                <th key={c} role="columnheader" aria-sort={sort?.[0] === c ? (sort[1] ? "ascending" : "descending") : "none"}>
                  <button type="button" className="sort" onClick={() => sortBy(c)}>
                    {pk.includes(c) && <span className="pk">PK</span>}
                    {c}
                    {sort?.[0] === c && (sort[1] ? " ▲" : " ▼")}
                  </button>
                  <ResizeHandle
                    label={c}
                    width={widths[resultColumns.indexOf(c)] ?? 160}
                    onResize={(w) => setWidth(resultColumns.indexOf(c), w)}
                    onFit={() => {
                      const k = resultColumns.indexOf(c);
                      setWidth(k, fitWidth(c, rows.map((_, r) => valueOf(r, c))));
                    }}
                  />
                </th>
              ))}
            </tr>
            {showFilters && (
              <tr className="filters">
                <th aria-hidden />
                {resultColumns.map((c) => {
                  const f = filterDraft[c] ?? { op: "", value: "" };
                  const needsValue = f.op !== "" && f.op !== "IsNull" && f.op !== "IsNotNull";
                  return (
                    <th key={c}>
                      <select
                        aria-label={`Filter ${c}`}
                        value={f.op}
                        onChange={(e) => setFilterDraft({ ...filterDraft, [c]: { ...f, op: e.target.value as FilterOp | "" } })}
                      >
                        {OPS.map((o) => (
                          <option key={o.op} value={o.op}>
                            {o.label}
                          </option>
                        ))}
                      </select>
                      {needsValue && (
                        <input
                          aria-label={`Filter ${c} value`}
                          value={f.value}
                          onChange={(e) => setFilterDraft({ ...filterDraft, [c]: { ...f, value: e.target.value } })}
                          onKeyDown={(e) => e.key === "Enter" && applyFilters()}
                        />
                      )}
                    </th>
                  );
                })}
              </tr>
            )}
          </thead>
          <tbody>
            {rows.map((_, i) => (
              <tr key={i} role="row" className={deleted.has(i) ? "deleted" : undefined}>
                <td className="rownum">
                  {editable || rows.length ? (
                    <input
                      type="checkbox"
                      aria-label={`Select row ${i + 1}`}
                      checked={selected.has(i)}
                      onChange={() => toggleSelected(i)}
                    />
                  ) : null}
                  <span>{page.offset + i + 1}</span>
                </td>
                {resultColumns.map((c) =>
                  cell({ row: i, added: false, column: c }, valueOf(i, c), !!edits.get(i) && c in (edits.get(i) as Values)),
                )}
              </tr>
            ))}
            {added.map((r, i) => (
              <tr key={`added-${i}`} role="row" className="added">
                <td className="rownum">
                  <button
                    type="button"
                    className="link subtle"
                    aria-label={`Remove new row ${i + 1}`}
                    onClick={() => setAdded((a) => a.filter((_, k) => k !== i))}
                  >
                    ×
                  </button>
                </td>
                {resultColumns.map((c) =>
                  c in r ? (
                    cell({ row: i, added: true, column: c }, r[c], true)
                  ) : (
                    <td
                      key={c}
                      data-column={c}
                      role="gridcell"
                      className="default"
                      onDoubleClick={() => startEdit({ row: i, added: true, column: c })}
                    >
                      default
                    </td>
                  ),
                )}
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="pager">
        <button
          type="button"
          className="ghost"
          disabled={page.offset === 0 || loading}
          onClick={() => changePage({ offset: Math.max(0, page.offset - PAGE_SIZE) })}
        >
          Previous
        </button>
        <span className="muted">
          {loading
            ? "Loading…"
            : rows.length
              ? `Rows ${page.offset + 1}–${page.offset + rows.length}`
              : "No rows"}
        </span>
        <button
          type="button"
          className="ghost"
          disabled={rows.length < PAGE_SIZE || loading}
          onClick={() => changePage({ offset: page.offset + PAGE_SIZE })}
        >
          Next
        </button>
      </div>

      {preview && (
        <SaveDialog
          batches={preview}
          production={production}
          busy={saving}
          onApply={apply}
          onCancel={() => setPreview(null)}
        />
      )}
    </div>
  );
}
