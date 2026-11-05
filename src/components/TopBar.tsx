import logo from "../../assets/brand/arco-logo.svg";
import type { SavedConnection, SchemaInfo } from "../types";

/** Connection, role, schema switcher, and the PRODUCTION and read-only markers. */
export default function TopBar({
  saved,
  user,
  schema,
  schemas,
  onSchema,
  onToggleHistory,
  historyOpen,
  onDisconnect,
}: {
  saved: SavedConnection;
  user: string;
  schema: string | null;
  schemas: SchemaInfo[];
  onSchema: (schema: string) => void;
  onToggleHistory: () => void;
  historyOpen: boolean;
  onDisconnect: () => void;
}) {
  return (
    <header className={`top workspace-top${saved.production ? " is-production" : ""}`}>
      <img src={logo} alt="" width={24} height={24} />
      <strong className="conn-name">{saved.name}</strong>
      <span className="muted">as {user}</span>
      <label className="schema-pick">
        <span className="muted">schema</span>
        <select value={schema ?? ""} onChange={(e) => onSchema(e.target.value)}>
          {!schema && <option value="">(pick one)</option>}
          {schemas.map((s) => (
            <option key={s.name} value={s.name}>
              {s.name}
            </option>
          ))}
        </select>
      </label>
      {saved.production && <span className="badge production">PRODUCTION</span>}
      {saved.read_only && <span className="badge">read-only</span>}
      <span className="spacer" />
      <button type="button" className="ghost" aria-pressed={historyOpen} onClick={onToggleHistory}>
        History
      </button>
      <button type="button" className="ghost" onClick={onDisconnect}>
        Disconnect
      </button>
    </header>
  );
}
