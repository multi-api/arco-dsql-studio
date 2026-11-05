import { useEffect, useState } from "react";

import { HISTORY_SETTING, clearHistory, getSetting, searchHistory, setSetting } from "../api";
import { errorText } from "../errors";
import type { HistoryEntry } from "../types";

/** Every statement run, newest first, searchable; clicking one opens it in a new tab. */
export default function HistoryPanel({ onOpen, refresh }: { onOpen: (sql: string) => void; refresh: number }) {
  const [query, setQuery] = useState("");
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [problem, setProblem] = useState<string | null>(null);
  const [enabled, setEnabled] = useState(true);
  const [confirmClear, setConfirmClear] = useState(false);

  useEffect(() => {
    getSetting(HISTORY_SETTING)
      .then((v) => setEnabled(v !== "off"))
      .catch(() => {});
  }, []);

  useEffect(() => {
    const t = setTimeout(() => {
      searchHistory(query)
        .then((list) => {
          setEntries(list);
          setProblem(null);
        })
        .catch((err) => setProblem(errorText(err)));
    }, 150);
    return () => clearTimeout(t);
  }, [query, refresh]);

  async function toggle(on: boolean) {
    setEnabled(on);
    await setSetting(HISTORY_SETTING, on ? "on" : "off").catch((err) => setProblem(errorText(err)));
  }

  async function clear() {
    setConfirmClear(false);
    try {
      await clearHistory();
      setEntries([]);
    } catch (err) {
      setProblem(errorText(err));
    }
  }

  return (
    <aside className="history" aria-label="History">
      <div className="history-head">
        <h3>History</h3>
        <label className="check small">
          <input type="checkbox" checked={enabled} onChange={(e) => toggle(e.target.checked)} />
          Keep history
        </label>
      </div>
      <input
        type="search"
        placeholder="Search"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        aria-label="Search history"
      />
      {problem && <p className="error-text">{problem}</p>}
      <ul className="history-list">
        {entries.map((e) => (
          <li key={e.id ?? e.started_at}>
            <button type="button" className="history-item" onClick={() => onOpen(e.sql)}>
              <code>{e.sql}</code>
              <span className={e.error ? "error-text" : "muted"}>
                {new Date(e.started_at).toLocaleString()} · {e.duration_ms} ms
                {e.error ? " · failed" : e.rows !== null ? ` · ${e.rows} rows` : ""}
              </span>
            </button>
          </li>
        ))}
        {entries.length === 0 && !problem && <li className="muted">Nothing yet.</li>}
      </ul>
      <div className="history-foot">
        {confirmClear ? (
          <>
            <span>Delete all history?</span>
            <button type="button" className="danger small" onClick={clear}>
              Delete
            </button>
            <button type="button" className="link subtle" onClick={() => setConfirmClear(false)}>
              Cancel
            </button>
          </>
        ) : (
          entries.length > 0 && (
            <button type="button" className="link subtle" onClick={() => setConfirmClear(true)}>
              Clear history
            </button>
          )
        )}
      </div>
    </aside>
  );
}
