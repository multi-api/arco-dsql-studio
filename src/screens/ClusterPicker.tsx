import { useCallback, useEffect, useState } from "react";

import { callerIdentity, getSetting, listClusters, parseEndpoint } from "../api";
import ExplainBox from "../components/ExplainBox";
import { explainOffline } from "../errors";
import type { CallerIdentity, ClusterInfo, Explanation, ProfileInfo } from "../types";

export interface ClusterChoice {
  identifier: string;
  region: string;
  host: string;
  tags: Record<string, string>;
}

/** Remembered regions, newest first; the next launch scans only these. */
export const LAST_REGIONS = "last_regions";

/** The role name in an assumed-role ARN: `.../assumed-role/<role>/<session>`. */
function roleOf(arn: string): string {
  const m = arn.match(/assumed-role\/([^/]+)/);
  return m ? m[1] : arn;
}

/** The profile's DSQL clusters grouped by region, plus manual endpoint entry. */
export default function ClusterPicker({
  profile,
  onPick,
  onBack,
  onSignIn,
}: {
  profile: ProfileInfo;
  onPick: (cluster: ClusterChoice) => void;
  onBack: () => void;
  onSignIn: () => void;
}) {
  const [me, setMe] = useState<CallerIdentity | null>(null);
  const [clusters, setClusters] = useState<ClusterInfo[] | null>(null);
  const [scanned, setScanned] = useState<"remembered" | "all">("all");
  const [unchecked, setUnchecked] = useState<string[]>([]);
  const [problem, setProblem] = useState<Explanation | null>(null);
  const [host, setHost] = useState("");
  const [hostError, setHostError] = useState<string | null>(null);

  const scan = useCallback(
    async (all: boolean) => {
      setProblem(null);
      setClusters(null);
      try {
        const identity = callerIdentity(profile.name, profile.region);
        const remembered = all ? null : await getSetting(LAST_REGIONS);
        const regions: string[] | null = remembered ? JSON.parse(remembered) : null;
        setScanned(regions ? "remembered" : "all");
        const list = await listClusters(profile.name, regions);
        setMe(await identity);
        setClusters(list.clusters);
        // Access denied and "not available here" are normal for regions nobody uses; only other failures are worth
        // a note.
        setUnchecked(
          list.failed
            .filter((f) => {
              const e = f.error;
              return !(typeof e === "object" && "Aws" in e && e.Aws.kind !== "Other" && e.Aws.kind !== "SsoExpired");
            })
            .map((f) => f.region),
        );
      } catch (err) {
        setProblem(await explainOffline(null, err));
      }
    },
    [profile],
  );

  useEffect(() => {
    scan(false);
  }, [scan]);

  async function useEndpoint() {
    setHostError(null);
    const parsed = await parseEndpoint(host).catch(() => null);
    if (!parsed) {
      setHostError("That is not a DSQL endpoint. It looks like <cluster-id>.dsql.<region>.on.aws.");
      return;
    }
    const [identifier, region] = parsed;
    onPick({ identifier, region, host: host.trim().toLowerCase(), tags: {} });
  }

  const byRegion = new Map<string, ClusterInfo[]>();
  for (const c of clusters ?? []) byRegion.set(c.region, [...(byRegion.get(c.region) ?? []), c]);

  return (
    <section className="panel">
      <div className="panel-head">
        <h2>Clusters for {profile.name}</h2>
        <button type="button" className="ghost" onClick={onBack}>
          Back
        </button>
      </div>
      {me && (
        <p className="muted">
          Signed in as <strong>{roleOf(me.arn)}</strong> in account <strong>{me.account}</strong>
        </p>
      )}

      {problem && <ExplainBox explanation={problem} onSignIn={onSignIn} onRetry={() => scan(false)} />}

      {!problem && clusters === null && <p>Looking for clusters…</p>}

      {clusters !== null && (
        <>
          {clusters.length === 0 && (
            <p>
              No clusters found{scanned === "remembered" ? " in the regions you used last" : ""}.
            </p>
          )}
          {[...byRegion.entries()].map(([region, list]) => (
            <div key={region} className="group">
              <h3>{region}</h3>
              <ul className="list">
                {list.map((c) => (
                  <li key={c.identifier}>
                    <button
                      type="button"
                      className="row"
                      onClick={() => onPick({ identifier: c.identifier, region: c.region, host: c.endpoint, tags: c.tags })}
                    >
                      <span>
                        <strong>{c.tags.Name ?? c.identifier}</strong>
                        {c.tags.Name && <span className="muted"> {c.identifier}</span>}
                        <span className="tags">
                          {Object.entries(c.tags)
                            .filter(([k]) => k !== "Name")
                            .map(([k, v]) => (
                              <span key={k} className="badge">
                                {k}: {v}
                              </span>
                            ))}
                        </span>
                      </span>
                      <span className={`status ${c.status === "ACTIVE" ? "ok" : ""}`}>{c.status}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ))}
          {unchecked.length > 0 && (
            <p className="muted">Could not check {unchecked.join(", ")}.</p>
          )}
          <div className="actions">
            <button type="button" className="ghost" onClick={() => scan(true)}>
              Scan all regions
            </button>
          </div>
        </>
      )}

      <form
        className="manual"
        onSubmit={(e) => {
          e.preventDefault();
          useEndpoint();
        }}
      >
        <label>
          Or enter an endpoint
          <input
            value={host}
            onChange={(e) => setHost(e.target.value)}
            placeholder="<cluster-id>.dsql.<region>.on.aws"
          />
        </label>
        <button type="submit" className="ghost" disabled={!host.trim()}>
          Use endpoint
        </button>
        {hostError && <p className="error-text">{hostError}</p>}
      </form>
    </section>
  );
}
