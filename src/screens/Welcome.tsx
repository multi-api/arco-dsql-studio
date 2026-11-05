import { useEffect, useState } from "react";

import { listConnections, listProfiles } from "../api";
import ExplainBox from "../components/ExplainBox";
import { explainOffline } from "../errors";
import type { Explanation, ProfileInfo, ProfileKind, SavedConnection } from "../types";

const KIND_LABEL: Record<ProfileKind, string> = {
  Sso: "SSO",
  AccessKeys: "Access keys",
  AssumeRole: "Role",
  Process: "Process",
  Other: "Other",
};

/** "Who are you on AWS?": the profiles in ~/.aws, saved connections, and the way to set up AWS access. */
export default function Welcome({
  onPickProfile,
  onSignIn,
  onSetup,
  onReconnect,
  problem: outerProblem,
}: {
  onPickProfile: (profile: ProfileInfo) => void;
  onSignIn: (profile: ProfileInfo) => void;
  onSetup: () => void;
  onReconnect: (saved: SavedConnection) => void;
  problem?: Explanation | null;
}) {
  const [profiles, setProfiles] = useState<ProfileInfo[] | null>(null);
  const [saved, setSaved] = useState<SavedConnection[]>([]);
  const [problem, setProblem] = useState<Explanation | null>(null);

  useEffect(() => {
    listProfiles()
      .then(setProfiles)
      .catch(async (err) => {
        setProfiles([]);
        setProblem(await explainOffline(null, err));
      });
    listConnections()
      .then(setSaved)
      .catch(() => setSaved([]));
  }, []);

  return (
    <section className="panel">
      {saved.length > 0 && (
        <div className="group">
          <h2>Your connections</h2>
          <ul className="list">
            {saved.map((c) => (
              <li key={c.id ?? c.name}>
                <button type="button" className="row" onClick={() => onReconnect(c)}>
                  <span>
                    <strong>{c.name}</strong>
                    <span className="muted">
                      {" "}
                      {c.role} · {c.region} · {c.profile}
                    </span>
                  </span>
                  <span className="tags">
                    {c.production && <span className="badge warn">production</span>}
                    {c.read_only && <span className="badge">read-only</span>}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {outerProblem && <ExplainBox explanation={outerProblem} />}

      <div className="group">
        <h2>Who are you on AWS?</h2>
        {profiles === null && <p>Reading ~/.aws…</p>}
        {profiles?.length === 0 && !problem && (
          <p>
            No AWS profiles on this computer yet. Set up access with your organisation's AWS access portal; the app adds
            a profile to ~/.aws/config that the AWS CLI can use too.
          </p>
        )}
        <ul className="list">
          {profiles?.map((p) => (
            <li key={p.name} className="row-with-action">
              <button type="button" className="row" onClick={() => onPickProfile(p)}>
                <span>
                  <strong>{p.name}</strong>
                  {p.kind === "Sso" && p.sso_account_id && (
                    <span className="muted">
                      {" "}
                      {p.sso_role_name} in {p.sso_account_id}
                    </span>
                  )}
                </span>
                <span className="badge">{KIND_LABEL[p.kind]}</span>
              </button>
              {p.kind === "Sso" && (
                <button type="button" className="ghost" onClick={() => onSignIn(p)}>
                  Sign in
                </button>
              )}
            </li>
          ))}
        </ul>
        {problem && <ExplainBox explanation={problem} />}
        <div className="actions">
          <button type="button" className={profiles?.length ? "ghost" : ""} onClick={onSetup}>
            Set up AWS access
          </button>
        </div>
      </div>
    </section>
  );
}
