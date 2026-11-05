import { openInBrowser } from "../browser";
import { useEffect, useState } from "react";

import { ssoFinishLogin, ssoStartLogin, ssoStoreLogin } from "../api";
import { explainOffline } from "../errors";
import type { Explanation, ProfileInfo, SsoLoginStarted } from "../types";
import DeviceCode from "./DeviceCode";
import ExplainBox from "./ExplainBox";

/** Signs an existing SSO profile in, like `aws sso login --profile`, and saves the session for the AWS CLI too. */
export default function SignIn({
  profile,
  onDone,
  onCancel,
}: {
  profile: ProfileInfo;
  onDone: () => void;
  onCancel: () => void;
}) {
  const [login, setLogin] = useState<SsoLoginStarted | null>(null);
  const [problem, setProblem] = useState<Explanation | null>(null);

  useEffect(() => {
    let live = true;
    (async () => {
      if (!profile.sso_start_url || !profile.sso_region) {
        setProblem({
          title: "This profile has no SSO details",
          body: "Its start URL or SSO region is missing in ~/.aws/config.",
          fix: "None",
        });
        return;
      }
      try {
        const started = await ssoStartLogin(profile.sso_start_url, profile.sso_region);
        if (!live) return;
        setLogin(started);
        await openInBrowser(started.verification_uri_complete);
        await ssoFinishLogin(started.login_id);
        await ssoStoreLogin(started.login_id, profile.sso_session);
        if (live) onDone();
      } catch (err) {
        if (live) setProblem(await explainOffline(null, err));
      }
    })();
    return () => {
      live = false;
    };
  }, [profile, onDone]);

  return (
    <section className="panel">
      <h2>Sign in to {profile.name}</h2>
      {problem ? <ExplainBox explanation={problem} /> : login ? <DeviceCode login={login} /> : <p>Starting…</p>}
      <div className="actions">
        <button type="button" className="ghost" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </section>
  );
}
