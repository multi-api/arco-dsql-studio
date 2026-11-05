import { useState } from "react";

import {
  getSetting,
  listSchemas,
  looksLikeProduction,
  openConnection,
  saveConnection,
  setSchema,
  setSetting,
} from "../api";
import ExplainBox from "../components/ExplainBox";
import { explainOffline } from "../errors";
import type { ConnectionHandle, Explanation, SavedConnection, SchemaInfo } from "../types";
import { type ClusterChoice, LAST_REGIONS } from "./ClusterPicker";

/** Pick who to connect as, connect, pick the schema if it is not clear, and save the connection. */
export default function Connect({
  profile,
  cluster,
  onConnected,
  onBack,
}: {
  profile: string;
  cluster: ClusterChoice;
  onConnected: (handle: ConnectionHandle, saved: SavedConnection) => void;
  onBack: () => void;
}) {
  const [asAdmin, setAsAdmin] = useState(true);
  const [roleName, setRoleName] = useState("");
  const [readOnly, setReadOnly] = useState(false);
  const [production, setProduction] = useState(false);
  const [handle, setHandle] = useState<ConnectionHandle | null>(null);
  const [schemas, setSchemas] = useState<SchemaInfo[] | null>(null);
  const [schema, setChosenSchema] = useState<string | null>(null);
  const [problem, setProblem] = useState<Explanation | null>(null);
  const [busy, setBusy] = useState(false);

  const role = asAdmin ? "admin" : roleName.trim();
  const saved = (schemaName: string | null): SavedConnection => ({
    id: null,
    name: `${cluster.tags.Name ?? cluster.identifier} · ${schemaName ?? role}`,
    profile,
    region: cluster.region,
    cluster_id: cluster.identifier,
    host: cluster.host,
    role,
    schema: schemaName,
    production,
    read_only: readOnly,
  });

  async function useSchema(name: string) {
    setChosenSchema(name);
    setProduction(await looksLikeProduction(name, cluster.tags).catch(() => false));
  }

  async function connect() {
    setProblem(null);
    setBusy(true);
    try {
      const h = await openConnection(saved(null));
      setHandle(h);
      if (h.schema) {
        await useSchema(h.schema);
      } else {
        setSchemas(await listSchemas(h.id));
      }
    } catch (err) {
      setProblem(await explainOffline(saved(null), err));
    } finally {
      setBusy(false);
    }
  }

  async function pickSchema(name: string) {
    if (!handle) return;
    try {
      await setSchema(handle.id, name);
      await useSchema(name);
    } catch (err) {
      setProblem(await explainOffline(saved(name), err));
    }
  }

  async function save() {
    if (!handle) return;
    setBusy(true);
    try {
      const connection = saved(schema);
      const id = await saveConnection(connection);
      const last: string[] = JSON.parse((await getSetting(LAST_REGIONS)) ?? "[]");
      await setSetting(LAST_REGIONS, JSON.stringify([cluster.region, ...last.filter((r) => r !== cluster.region)]));
      onConnected({ ...handle, schema }, { ...connection, id });
    } catch (err) {
      setProblem(await explainOffline(saved(schema), err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel">
      <div className="panel-head">
        <h2>Connect to {cluster.tags.Name ?? cluster.identifier}</h2>
        <button type="button" className="ghost" onClick={onBack}>
          Back
        </button>
      </div>
      <p className="muted">
        {cluster.host} · {profile}
      </p>

      {!handle && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            connect();
          }}
        >
          <fieldset>
            <legend>Connect as</legend>
            <label className="choice">
              <input type="radio" name="role" checked={asAdmin} onChange={() => setAsAdmin(true)} />
              <span>
                <strong>admin</strong>
                <br />
                <span className="muted">
                  Needs dsql:DbConnectAdmin. Sees everything, but cannot read an app's tables until granted.
                </span>
              </span>
            </label>
            <label className="choice">
              <input
                type="radio"
                name="role"
                checked={!asAdmin}
                onChange={() => setAsAdmin(false)}
                aria-label="A custom role"
              />
              <span>
                <strong>A custom role</strong> (for example readonly)
                <br />
                <span className="muted">Needs dsql:DbConnect and an AWS IAM GRANT mapping to your IAM role.</span>
              </span>
            </label>
            {!asAdmin && (
              <label>
                Role name
                <input value={roleName} onChange={(e) => setRoleName(e.target.value)} required />
              </label>
            )}
          </fieldset>
          <label className="check">
            <input type="checkbox" checked={readOnly} onChange={(e) => setReadOnly(e.target.checked)} />
            Read-only: only run queries that read
          </label>
          <div className="actions">
            <button type="submit" disabled={busy || !role}>
              {busy ? "Connecting…" : "Connect"}
            </button>
          </div>
        </form>
      )}

      {handle && !schema && schemas && (
        <fieldset>
          <legend>Pick the schema</legend>
          <p className="muted">Queries run in this schema, so select * from users just works.</p>
          {schemas.map((s) => (
            <label key={s.name} className="choice">
              <input type="radio" name="schema" onChange={() => pickSchema(s.name)} aria-label={s.name} />
              <span>
                <strong>{s.name}</strong> <span className="muted">owner {s.owner}</span>
              </span>
            </label>
          ))}
          {schemas.length === 0 && <p>This role cannot see any schema.</p>}
        </fieldset>
      )}

      {handle && schema && (
        <div>
          <p>
            Connected as <strong>{handle.current_user}</strong> in schema <strong>{schema}</strong>.
          </p>
          <label className="check">
            <input
              type="checkbox"
              checked={production}
              onChange={(e) => setProduction(e.target.checked)}
              aria-label="This is production"
            />
            This is production: confirm before anything that writes or changes the schema
          </label>
          <div className="actions">
            <button type="button" disabled={busy} onClick={save}>
              Save and open
            </button>
          </div>
        </div>
      )}

      {problem && <ExplainBox explanation={problem} connectionId={handle?.id} onRetry={connect} />}
    </section>
  );
}
