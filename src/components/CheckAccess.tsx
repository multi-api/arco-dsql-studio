import { useEffect, useState } from "react";

import { listTables } from "../api";
import { errorText } from "../errors";
import type { TableInfo } from "../types";

const q = (name: string) => `"${name.replace(/"/g, '""')}"`;

const PRIVILEGES = [
  ["SELECT", "can_select"],
  ["INSERT", "can_insert"],
  ["UPDATE", "can_update"],
  ["DELETE", "can_delete"],
] as const;

/** The GRANT that gives `role` what it is missing on `t`, or null when it has everything. */
export function missingGrant(t: TableInfo, role: string): string | null {
  const missing = PRIVILEGES.filter(([, key]) => !t[key]).map(([name]) => name);
  return missing.length ? `GRANT ${missing.join(", ")} ON ${q(t.schema)}.${q(t.name)} TO ${q(role)};` : null;
}

/** What the current role can do with each table of the schema, and the grant its owner would run for the rest. */
export default function CheckAccess({ connectionId, schema, role }: { connectionId: string; schema: string; role: string }) {
  const [tables, setTables] = useState<TableInfo[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    listTables(connectionId, schema)
      .then((list) => setTables(list.filter((t) => t.kind !== "Sequence")))
      .catch((err) => setProblem(errorText(err)));
  }, [connectionId, schema]);

  const grants = (tables ?? []).map((t) => missingGrant(t, role)).filter((g): g is string => g !== null);

  return (
    <div className="tool">
      <h2>
        Access for {role} in {schema}
      </h2>
      {problem && <p className="error-text">{problem}</p>}
      {tables === null && !problem && <p className="muted">Reading privileges…</p>}
      {tables && tables.length === 0 && <p>This schema has no tables.</p>}
      {tables && tables.length > 0 && (
        <table className="access">
          <thead>
            <tr>
              <th>Table</th>
              <th>Owner</th>
              {PRIVILEGES.map(([name]) => (
                <th key={name}>{name}</th>
              ))}
              <th>To grant the rest, the owner runs</th>
            </tr>
          </thead>
          <tbody>
            {tables.map((t) => {
              const grant = missingGrant(t, role);
              return (
                <tr key={t.name}>
                  <td>{t.name}</td>
                  <td className="muted">{t.owner}</td>
                  {PRIVILEGES.map(([name, key]) => (
                    <td key={name} className={t[key] ? "yes" : "no"} aria-label={`${name} ${t[key] ? "yes" : "no"}`}>
                      {t[key] ? "✓" : "✗"}
                    </td>
                  ))}
                  <td>{grant ? <code>{grant}</code> : <span className="muted">nothing missing</span>}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      {grants.length > 0 && (
        <div className="actions">
          <button type="button" className="ghost" onClick={() => navigator.clipboard.writeText(grants.join("\n")).catch(() => {})}>
            Copy all grants
          </button>
          <span className="muted">Only each table's owner can run these. The team access wizard can help.</span>
        </div>
      )}
    </div>
  );
}
