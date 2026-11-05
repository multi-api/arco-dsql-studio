import { useEffect, useState } from "react";

import {
  closeConnection,
  iamRoleForProfile,
  listTables,
  openConnection,
  roleAdminCanTakeOver,
  roleConsoleFunctionName,
  roleCreatePlan,
  roleMappings,
  roleRemovePlan,
  roleRunAdmin,
  roleRunOwnerViaConsole,
  roleTakeoverPlan,
} from "../api";
import { errorText } from "../errors";
import type { Access, RolePlan, SavedConnection, ScriptResult, TableInfo } from "../types";

type Step = "access" | "iam" | "name" | "review" | "owner" | "test" | "done";

/** The error a run of wizard SQL ended with, as text, or null when every statement ran. */
function failure(result: ScriptResult): string | null {
  for (const o of result.outcomes) if ("Error" in o) return errorText(o.Error);
  return null;
}

function copy(text: string) {
  navigator.clipboard.writeText(text).catch(() => {});
}

function Sql({ lines }: { lines: string[] }) {
  return <pre className="statements">{lines.map((l) => l + ";").join("\n")}</pre>;
}

/**
 * "Give my team access": create a DSQL role mapped to an IAM role, with the admin SQL run here and the owner grants run
 * by whoever can (the console Lambda, a pasted snippet, or admin through temporary membership); or remove one in the
 * order DSQL accepts. Every statement is shown before it runs.
 */
export default function RoleWizard({
  mode,
  connectionId,
  saved,
  schema,
  onSaveConnection,
}: {
  mode: "create" | "remove";
  connectionId: string;
  saved: SavedConnection;
  schema: string;
  onSaveConnection: (connection: SavedConnection) => void;
}) {
  const [step, setStep] = useState<Step>(mode === "create" ? "access" : "name");
  const [access, setAccess] = useState<Access>("ReadOnly");
  const [arn, setArn] = useState("");
  const [arnGuessed, setArnGuessed] = useState(false);
  const [role, setRole] = useState(mode === "create" ? "readonly" : "");
  const [tables, setTables] = useState<TableInfo[]>([]);
  const [mappings, setMappings] = useState<[string, string][]>([]);
  const [plan, setPlan] = useState<RolePlan | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [ownerDone, setOwnerDone] = useState(false);
  const [tested, setTested] = useState<string | null>(null);

  const owner = tables[0]?.owner ?? "";
  const tableNames = tables.filter((t) => t.kind !== "Sequence").map((t) => t.name);

  useEffect(() => {
    listTables(connectionId, schema)
      .then(setTables)
      .catch((err) => setProblem(errorText(err)));
    if (mode === "remove") {
      roleMappings(connectionId)
        .then(setMappings)
        .catch((err) => setProblem(errorText(err)));
    }
  }, [connectionId, schema, mode]);

  async function attempt(work: () => Promise<void>) {
    setProblem(null);
    setBusy(true);
    try {
      await work();
    } catch (err) {
      setProblem(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  const loadArn = () =>
    attempt(async () => {
      setStep("iam");
      const found = await iamRoleForProfile(saved.profile, saved.region);
      setArn(found.arn);
      setArnGuessed(found.guessed);
    });

  const review = () =>
    attempt(async () => {
      if (mode === "create") {
        setPlan(await roleCreatePlan(role.trim(), arn.trim(), schema, tableNames, access));
      } else {
        const mapped = mappings.find(([r]) => r === role);
        setPlan(await roleRemovePlan(role, mapped?.[1] ?? arn.trim(), schema, tableNames));
      }
      setStep("review");
    });

  const runAdmin = () =>
    attempt(async () => {
      if (!plan) return;
      const result = await roleRunAdmin(connectionId, plan.admin_sql);
      const err = failure(result);
      if (err) throw new Error(err);
      setStep(mode === "create" ? "owner" : "done");
    });

  const testRole = () =>
    attempt(async () => {
      const connection: SavedConnection = {
        ...saved,
        id: null,
        name: `${saved.name.split(" · ")[0]} · ${role}`,
        role: role.trim(),
        schema,
        read_only: access === "ReadOnly",
      };
      const handle = await openConnection(connection);
      const visible = await listTables(handle.id, schema).finally(() => closeConnection(handle.id).catch(() => {}));
      const readable = visible.filter((t) => t.can_select).length;
      setTested(`Signed in as ${handle.current_user}: can read ${readable} of ${visible.length} tables in ${schema}.`);
      setStep("test");
    });

  return (
    <div className="tool wizard">
      <h2>{mode === "create" ? "Give your team access" : "Remove a role"}</h2>
      <p className="muted">
        In DSQL an app's tables belong to the app's own role, which nobody can sign in as. This creates a role your IAM
        role can sign in with, and grants it access to the tables in <strong>{schema}</strong>.
      </p>

      {mode === "create" && step === "access" && (
        <fieldset>
          <legend>1. What should the role be able to do?</legend>
          {(["ReadOnly", "ReadWrite"] as Access[]).map((a) => (
            <label key={a} className="choice">
              <input type="radio" name="access" checked={access === a} onChange={() => {
                setAccess(a);
                setRole(a === "ReadOnly" ? "readonly" : "team_rw");
              }} />
              <span>
                <strong>{a === "ReadOnly" ? "Read only" : "Read and write"}</strong>
                <br />
                <span className="muted">{a === "ReadOnly" ? "SELECT" : "SELECT, INSERT, UPDATE, DELETE"} on every table</span>
              </span>
            </label>
          ))}
          <div className="actions">
            <button type="button" onClick={loadArn}>
              Continue
            </button>
          </div>
        </fieldset>
      )}

      {mode === "create" && step === "iam" && (
        <form onSubmit={(e) => { e.preventDefault(); setStep("name"); }}>
          <label>
            2. IAM role ARN to map
            <input value={arn} onChange={(e) => setArn(e.target.value)} placeholder="arn:aws:iam::<account>:role/<path>/<name>" required />
          </label>
          <p className="muted">
            {arnGuessed
              ? "Built from the SSO path pattern because iam:GetRole was not allowed. Check it in the IAM console."
              : "The IAM role behind your profile. Anyone who can use it will be able to sign in as the new role."}
          </p>
          <div className="actions">
            <button type="submit" disabled={busy || !arn.trim()}>
              Continue
            </button>
          </div>
        </form>
      )}

      {step === "name" && (
        <form onSubmit={(e) => { e.preventDefault(); review(); }}>
          {mode === "create" ? (
            <label>
              3. Role name
              <input value={role} onChange={(e) => setRole(e.target.value)} required />
            </label>
          ) : (
            <label>
              Role to remove
              <select value={role} onChange={(e) => setRole(e.target.value)} required>
                <option value="">(pick one)</option>
                {mappings.map(([r, a]) => (
                  <option key={`${r}-${a}`} value={r}>
                    {r} → {a.split("/").pop()}
                  </option>
                ))}
              </select>
            </label>
          )}
          {mode === "remove" && mappings.length === 0 && <p className="muted">No role is mapped to an IAM role.</p>}
          <div className="actions">
            <button type="submit" disabled={busy || !role.trim()}>
              Review
            </button>
          </div>
        </form>
      )}

      {plan && step === "review" && mode === "create" && (
        <div className="steps">
          <h3>Run as admin, here</h3>
          <Sql lines={plan.admin_sql} />
          <h3>Then, as the tables' owner ({owner || "the app's role"})</h3>
          <Sql lines={plan.owner_sql} />
          <div className="actions">
            <button type="button" onClick={runAdmin} disabled={busy}>
              Run the admin steps
            </button>
          </div>
        </div>
      )}

      {plan && mode === "create" && step === "owner" && (
        <OwnerStep
          connectionId={connectionId}
          saved={saved}
          schema={schema}
          owner={owner}
          plan={plan}
          done={ownerDone}
          onDone={() => setOwnerDone(true)}
          onProblem={setProblem}
        />
      )}
      {mode === "create" && step === "owner" && ownerDone && (
        <div className="actions">
          <button type="button" onClick={testRole} disabled={busy}>
            Test: sign in as {role}
          </button>
        </div>
      )}

      {plan && mode === "remove" && (step === "review" || step === "done") && (
        <div className="steps">
          <h3>1. As the tables' owner ({owner || "the app's role"}): revoke</h3>
          <OwnerStep
            connectionId={connectionId}
            saved={saved}
            schema={schema}
            owner={owner}
            plan={plan}
            done={ownerDone}
            onDone={() => setOwnerDone(true)}
            onProblem={setProblem}
          />
          <h3>2. As admin, here: unmap and drop</h3>
          <p className="muted">DROP ROLE fails while the role still holds privileges, so this comes after step 1.</p>
          <Sql lines={plan.admin_sql} />
          <div className="actions">
            <button type="button" onClick={runAdmin} disabled={busy || !ownerDone || step === "done"}>
              Remove the role
            </button>
          </div>
          {step === "done" && <p>Removed {role}. Its IAM mapping and schema access are gone.</p>}
        </div>
      )}

      {step === "test" && tested && (
        <div className="steps">
          <p>{tested}</p>
          <div className="actions">
            <button
              type="button"
              onClick={() => {
                onSaveConnection({
                  ...saved,
                  id: null,
                  name: `${saved.name.split(" · ")[0]} · ${role}`,
                  role: role.trim(),
                  schema,
                  read_only: access === "ReadOnly",
                });
                setStep("done");
              }}
            >
              Save as a connection
            </button>
          </div>
        </div>
      )}
      {mode === "create" && step === "done" && <p>Saved. It is under Your connections.</p>}

      {problem && (
        <p className="error-text" role="alert">
          {problem}
        </p>
      )}
    </div>
  );
}

/**
 * The owner-side SQL, which only the tables' owner can run. Choices: the Arco console Lambda, a tinker snippet, plain
 * SQL, or (only when admin holds ADMIN OPTION on the owner) admin through temporary membership, with explicit consent.
 */
function OwnerStep({
  connectionId,
  saved,
  schema,
  owner,
  plan,
  done,
  onDone,
  onProblem,
}: {
  connectionId: string;
  saved: SavedConnection;
  schema: string;
  owner: string;
  plan: RolePlan;
  done: boolean;
  onDone: () => void;
  onProblem: (problem: string | null) => void;
}) {
  const [how, setHow] = useState<"console" | "paste" | "admin">("console");
  const [canTakeOver, setCanTakeOver] = useState(false);
  const [fn, setFn] = useState("");
  const [output, setOutput] = useState<string | null>(null);
  const [consent, setConsent] = useState(false);
  const [takeover, setTakeover] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (owner) {
      roleAdminCanTakeOver(connectionId, owner)
        .then(setCanTakeOver)
        .catch(() => setCanTakeOver(false));
    }
    roleConsoleFunctionName(schema)
      .then(setFn)
      .catch(() => {});
  }, [connectionId, owner, schema]);

  useEffect(() => {
    if (how === "admin" && owner) {
      roleTakeoverPlan(owner, plan.owner_sql)
        .then(setTakeover)
        .catch(() => {});
    }
  }, [how, owner, plan]);

  async function run(work: () => Promise<void>) {
    onProblem(null);
    setBusy(true);
    try {
      await work();
      onDone();
    } catch (err) {
      onProblem(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="owner-step">
      <p>
        Only the tables' owner, <strong>{owner || "the app's role"}</strong>, can grant on its tables, and nobody can
        sign in as it. Pick how to run these statements.
      </p>
      <fieldset>
        <label className="choice">
          <input type="radio" name="owner-how" checked={how === "console"} onChange={() => setHow("console")} />
          <span>
            <strong>Run it for me</strong> (Arco Cloud apps)
            <br />
            <span className="muted">Runs the tinker snippet through the environment's console Lambda, with your profile.</span>
          </span>
        </label>
        <label className="choice">
          <input type="radio" name="owner-how" checked={how === "paste"} onChange={() => setHow("paste")} />
          <span>
            <strong>Copy and run it yourself</strong>
            <br />
            <span className="muted">In Arco Cloud's Run command or vendor/bin/arco tinker, or as SQL wherever the owner signs in.</span>
          </span>
        </label>
        {canTakeOver && (
          <label className="choice">
            <input
              type="radio"
              name="owner-how"
              checked={how === "admin"}
              onChange={() => setHow("admin")}
              aria-label="Let admin do it (temporary)"
            />
            <span>
              <strong>Let admin do it (temporary)</strong>
              <br />
              <span className="muted">admin created {owner}, so it can join that role for a moment.</span>
            </span>
          </label>
        )}
      </fieldset>

      {how === "console" && (
        <div className="steps">
          <label>
            Console function
            <input value={fn} onChange={(e) => setFn(e.target.value)} />
          </label>
          <p className="muted">Needs lambda:InvokeFunction on it. The code it runs:</p>
          <pre className="statements">{plan.owner_tinker}</pre>
          <div className="actions">
            <button
              type="button"
              disabled={busy || !fn.trim() || done}
              onClick={() =>
                run(async () => setOutput(await roleRunOwnerViaConsole(saved.profile, saved.region, fn.trim(), plan.owner_tinker)))
              }
            >
              Run in {fn || "the console"}
            </button>
          </div>
          {output !== null && <pre className="statements">{output || "(no output)"}</pre>}
        </div>
      )}

      {how === "paste" && (
        <div className="steps">
          <p className="muted">Tinker (Arco Cloud's Run command, or vendor/bin/arco tinker):</p>
          <div className="snippet">
            <pre>{plan.owner_tinker}</pre>
            <button type="button" className="ghost" onClick={() => copy(plan.owner_tinker)}>
              Copy
            </button>
          </div>
          <p className="muted">SQL:</p>
          <div className="snippet">
            <Sql lines={plan.owner_sql} />
            <button type="button" className="ghost" onClick={() => copy(plan.owner_sql.map((l) => l + ";").join("\n"))}>
              Copy
            </button>
          </div>
          <div className="actions">
            <button type="button" className="ghost" onClick={onDone} disabled={done}>
              The owner ran it
            </button>
          </div>
        </div>
      )}

      {how === "admin" && canTakeOver && (
        <div className="steps">
          <p>
            admin joins <strong>{owner}</strong>, runs the statements with its privileges, and leaves the role right
            after. DSQL has no SET ROLE; membership is how admin acts as the owner. If a statement fails, the last step
            still needs to run: the app shows it.
          </p>
          <Sql lines={takeover} />
          <label className="check">
            <input type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} aria-label="I agree" />
            I agree that admin acts as {owner} for these statements.
          </label>
          <div className="actions">
            <button
              type="button"
              disabled={busy || !consent || done || takeover.length === 0}
              onClick={() =>
                run(async () => {
                  const result = await roleRunAdmin(connectionId, takeover);
                  const err = failure(result);
                  if (err) {
                    // Never leave admin inside the owner's role.
                    await roleRunAdmin(connectionId, takeover.slice(-1)).catch(() => {});
                    throw new Error(err);
                  }
                })
              }
            >
              Grant as admin
            </button>
          </div>
        </div>
      )}

      {/* Escape hatch for every path: the console or snippet may have been run elsewhere. */}
      {how !== "paste" && !done && (
        <p className="muted">
          Ran it another way?{" "}
          <button type="button" className="link" onClick={onDone}>
            The owner ran it
          </button>
        </p>
      )}
      {done && <p className="ok-text">Owner step done.</p>}
    </div>
  );
}
