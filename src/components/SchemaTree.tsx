import { useEffect, useState } from "react";

import { listTables, tableColumns, tableIndexes } from "../api";
import { errorText } from "../errors";
import type { ColumnInfo, IndexInfo, SchemaInfo, TableInfo } from "../types";

interface TableDetail {
  columns: ColumnInfo[];
  indexes: IndexInfo[];
}

function privileges(t: TableInfo): string {
  const can = [
    t.can_select && "SELECT",
    t.can_insert && "INSERT",
    t.can_update && "UPDATE",
    t.can_delete && "DELETE",
  ].filter(Boolean);
  return `Owner: ${t.owner}\nYour role can: ${can.length ? can.join(", ") : "nothing"}`;
}

/**
 * Schemas → tables, views, sequences → columns and indexes, loaded as they are opened (reading a whole catalog takes
 * seconds on DSQL). Tables the role cannot read have a lock; hovering shows the owner and the role's privileges.
 */
export default function SchemaTree({
  connectionId,
  schemas,
  current,
  onOpenTable,
}: {
  connectionId: string;
  schemas: SchemaInfo[];
  current: string | null;
  onOpenTable: (schema: string, table: string) => void;
}) {
  const [open, setOpen] = useState<Set<string>>(() => new Set(current ? [current] : []));
  const [tables, setTables] = useState<Record<string, TableInfo[] | string>>({});
  const [details, setDetails] = useState<Record<string, TableDetail | string>>({});

  useEffect(() => {
    for (const schema of open) {
      if (schema.includes(".") || tables[schema] !== undefined) continue;
      setTables((t) => ({ ...t, [schema]: [] }));
      listTables(connectionId, schema)
        .then((list) => setTables((t) => ({ ...t, [schema]: list })))
        .catch((err) => setTables((t) => ({ ...t, [schema]: errorText(err) })));
    }
  }, [open, connectionId, tables]);

  function toggle(key: string) {
    setOpen((o) => {
      const next = new Set(o);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  function toggleTable(schema: string, table: string) {
    const key = `${schema}.${table}`;
    toggle(key);
    if (details[key] !== undefined) return;
    setDetails((d) => ({ ...d, [key]: { columns: [], indexes: [] } }));
    Promise.all([tableColumns(connectionId, schema, table), tableIndexes(connectionId, schema, table)])
      .then(([columns, indexes]) => setDetails((d) => ({ ...d, [key]: { columns, indexes } })))
      .catch((err) => setDetails((d) => ({ ...d, [key]: errorText(err) })));
  }

  return (
    <nav className="tree" aria-label="Schemas">
      <ul role="tree">
        {schemas.map((s) => {
          const list = tables[s.name];
          return (
            <li key={s.name} role="treeitem" aria-expanded={open.has(s.name)}>
              <button type="button" className="node" onClick={() => toggle(s.name)}>
                <span className="twisty">{open.has(s.name) ? "▾" : "▸"}</span>
                <span className={s.name === current ? "current" : ""}>{s.name}</span>
              </button>
              {open.has(s.name) && (
                <ul role="group">
                  {typeof list === "string" && <li className="tree-note">{list}</li>}
                  {Array.isArray(list) && list.length === 0 && <li className="tree-note">No tables</li>}
                  {Array.isArray(list) &&
                    list.map((t) => {
                      const key = `${s.name}.${t.name}`;
                      const detail = details[key];
                      return (
                        <li key={t.name} role="treeitem" aria-expanded={open.has(key)}>
                          <button
                            type="button"
                            className="node"
                            title={privileges(t)}
                            onClick={() => toggleTable(s.name, t.name)}
                            onDoubleClick={() => onOpenTable(s.name, t.name)}
                          >
                            <span className="twisty">{open.has(key) ? "▾" : "▸"}</span>
                            <span>{t.name}</span>
                            {t.kind !== "Table" && <span className="kind">{t.kind.toLowerCase()}</span>}
                            {!t.can_select && (
                              <span className="lock" aria-label="Your role cannot read this table">
                                🔒
                              </span>
                            )}
                          </button>
                          {open.has(key) && (
                            <ul role="group" className="columns">
                              {typeof detail === "string" && <li className="tree-note">{detail}</li>}
                              {typeof detail === "object" &&
                                detail.columns.map((c) => (
                                  <li
                                    key={c.name}
                                    title={`${c.data_type}${c.nullable ? "" : " NOT NULL"}${c.default ? ` DEFAULT ${c.default}` : ""}`}
                                  >
                                    {c.is_pk && <span className="pk">PK</span>}
                                    {c.name} <span className="muted">{c.data_type}</span>
                                  </li>
                                ))}
                              {typeof detail === "object" &&
                                detail.indexes.map((ix) => (
                                  <li key={ix.name} className="muted" title={ix.name}>
                                    {ix.unique ? "unique index" : "index"} ({ix.columns.join(", ")})
                                  </li>
                                ))}
                              <li>
                                <button type="button" className="link" onClick={() => onOpenTable(s.name, t.name)}>
                                  Query this table
                                </button>
                              </li>
                            </ul>
                          )}
                        </li>
                      );
                    })}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
