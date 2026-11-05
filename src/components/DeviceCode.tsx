import { openInBrowser } from "../browser";

import type { SsoLoginStarted } from "../types";

/** The SSO device code to confirm in the browser, like `aws sso login` shows. */
export default function DeviceCode({ login }: { login: SsoLoginStarted }) {
  return (
    <div className="device-code">
      <p>
        Your browser asks you to approve this sign-in. Check that it shows this code:
      </p>
      <p className="code">{login.user_code}</p>
      <div className="actions">
        <button type="button" onClick={() => openInBrowser(login.verification_uri_complete)}>
          Open the browser again
        </button>
      </div>
      <p className="muted">Waiting for approval… The code expires in {Math.round(login.expires_in / 60)} minutes.</p>
    </div>
  );
}
