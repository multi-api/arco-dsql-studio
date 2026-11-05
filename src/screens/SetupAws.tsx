import { openInBrowser } from "../browser";
import { useState } from "react";

import {
  appendSsoProfile,
  previewSsoProfile,
  ssoFinishLogin,
  ssoListAccounts,
  ssoStartLogin,
} from "../api";
import DeviceCode from "../components/DeviceCode";
import ExplainBox from "../components/ExplainBox";
import { explainOffline } from "../errors";
import type { Explanation, NewSsoProfile, SsoAccountRole, SsoLoginStarted } from "../types";

type Step = "portal" | "approve" | "account" | "profile" | "preview";

const slug = (s: string) =>
  s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");

/** `corp` from `https://corp.awsapps.com/start`; the `sso-session` name. */
function sessionName(startUrl: string): string {
  try {
    return slug(new URL(startUrl).hostname.split(".")[0]) || "sso";
  } catch {
    return "sso";
  }
}

/**
 * "Set up AWS access": sign in to the AWS access portal, pick an account and role, and add a profile to
 * ~/.aws/config after showing the exact lines. Existing lines are never changed.
 */
export default function SetupAws({ onDone, onCancel }: { onDone: (profile: string) => void; onCancel: () => void }) {
  const [step, setStep] = useState<Step>("portal");
  const [startUrl, setStartUrl] = useState("");
  const [ssoRegion, setSsoRegion] = useState("us-east-1");
  const [login, setLogin] = useState<SsoLoginStarted | null>(null);
  const [accounts, setAccounts] = useState<SsoAccountRole[]>([]);
  const [choice, setChoice] = useState<SsoAccountRole | null>(null);
  const [profile, setProfile] = useState<NewSsoProfile | null>(null);
  const [lines, setLines] = useState("");
  const [problem, setProblem] = useState<Explanation | null>(null);
  const [busy, setBusy] = useState(false);

  async function attempt(work: () => Promise<void>) {
    setProblem(null);
    setBusy(true);
    try {
      await work();
    } catch (err) {
      setProblem(await explainOffline(null, err));
    } finally {
      setBusy(false);
    }
  }

  const signIn = () =>
    attempt(async () => {
      const started = await ssoStartLogin(startUrl.trim(), ssoRegion.trim());
      setLogin(started);
      setStep("approve");
      await openInBrowser(started.verification_uri_complete);
      try {
        await ssoFinishLogin(started.login_id);
      } catch (err) {
        setStep("portal");
        throw err;
      }
      setAccounts(await ssoListAccounts(started.login_id));
      setStep("account");
    });

  function chooseAccount() {
    if (!choice) return;
    setProfile({
      profile: `${slug(choice.account_name) || choice.account_id}-${slug(choice.role_name)}`,
      session: sessionName(startUrl),
      start_url: startUrl.trim(),
      sso_region: ssoRegion.trim(),
      account_id: choice.account_id,
      role_name: choice.role_name,
      region: ssoRegion.trim(),
    });
    setStep("profile");
  }

  const preview = () =>
    attempt(async () => {
      if (!profile) return;
      setLines(await previewSsoProfile(profile));
      setStep("preview");
    });

  const confirm = () =>
    attempt(async () => {
      if (!profile || !login) return;
      await appendSsoProfile(profile, login.login_id);
      onDone(profile.profile);
    });

  return (
    <section className="panel">
      <h2>Set up AWS access</h2>

      {step === "portal" && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            signIn();
          }}
        >
          <p className="muted">
            Use your organisation's AWS access portal (IAM Identity Center). Its URL is in the invitation email or on
            the portal's sign-in page, and looks like https://your-org.awsapps.com/start.
          </p>
          <label>
            AWS access portal URL
            <input
              value={startUrl}
              onChange={(e) => setStartUrl(e.target.value)}
              placeholder="https://your-org.awsapps.com/start"
              required
            />
          </label>
          <label>
            SSO region
            <input value={ssoRegion} onChange={(e) => setSsoRegion(e.target.value)} required />
          </label>
          <div className="actions">
            <button type="submit" disabled={busy || !startUrl.trim()}>
              Sign in
            </button>
            <button type="button" className="ghost" onClick={onCancel}>
              Cancel
            </button>
          </div>
        </form>
      )}

      {step === "approve" && login && <DeviceCode login={login} />}

      {step === "account" && (
        <div>
          <p>Pick the account and role to use:</p>
          <div className="choices">
            {accounts.map((a) => {
              const id = `${a.account_id}/${a.role_name}`;
              return (
                <label key={id} className="choice">
                  <input
                    type="radio"
                    name="account"
                    checked={choice === a}
                    onChange={() => setChoice(a)}
                    aria-label={`${a.account_name} ${a.role_name}`}
                  />
                  <span>
                    <strong>{a.account_name}</strong> <span className="muted">{a.account_id}</span>
                    <br />
                    {a.role_name}
                  </span>
                </label>
              );
            })}
          </div>
          {accounts.length === 0 && <p className="muted">This sign-in cannot reach any account.</p>}
          <div className="actions">
            <button type="button" disabled={!choice} onClick={chooseAccount}>
              Continue
            </button>
            <button type="button" className="ghost" onClick={onCancel}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {step === "profile" && profile && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            preview();
          }}
        >
          <label>
            Profile name
            <input
              value={profile.profile}
              onChange={(e) => setProfile({ ...profile, profile: e.target.value })}
              required
            />
          </label>
          <label>
            Default region
            <input
              value={profile.region}
              onChange={(e) => setProfile({ ...profile, region: e.target.value })}
              required
            />
          </label>
          <div className="actions">
            <button type="submit" disabled={busy}>
              Preview
            </button>
            <button type="button" className="ghost" onClick={() => setStep("account")}>
              Back
            </button>
          </div>
        </form>
      )}

      {step === "preview" && (
        <div>
          <p>These lines will be added at the end of ~/.aws/config. Nothing already there changes.</p>
          <pre className="preview" data-testid="profile-preview">
            {lines}
          </pre>
          <div className="actions">
            <button type="button" disabled={busy} onClick={confirm}>
              Add to ~/.aws/config
            </button>
            <button type="button" className="ghost" onClick={() => setStep("profile")}>
              Back
            </button>
          </div>
        </div>
      )}

      {problem && <ExplainBox explanation={problem} />}
    </section>
  );
}
