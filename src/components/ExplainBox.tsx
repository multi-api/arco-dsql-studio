import { useState } from "react";

import { setSchema } from "../api";
import { errorText } from "../errors";
import type { Explanation } from "../types";

interface Props {
  explanation: Explanation;
  /** Needed for the "switch schema" fix. */
  connectionId?: string;
  onSchemaChanged?: (schema: string) => void;
  onSignIn?: () => void;
  onRetry?: () => void;
  onRollback?: () => void;
}

/** An error explained in plain language, with the one action that fixes it when there is one. */
export default function ExplainBox({
  explanation,
  connectionId,
  onSchemaChanged,
  onSignIn,
  onRetry,
  onRollback,
}: Props) {
  const { title, body, fix } = explanation;
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  async function switchSchema(schema: string) {
    if (!connectionId) return;
    setBusy(true);
    try {
      await setSchema(connectionId, schema);
      onSchemaChanged?.(schema);
    } catch (err) {
      setNote(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setNote("Copied.");
    } catch {
      setNote("Select the text and copy it.");
    }
  }

  return (
    <div className="explain" role="alert">
      <strong className="explain-title">{title}</strong>
      {body && <p className="explain-body">{body}</p>}
      {typeof fix === "object" && "ShowGrant" in fix && (
        <div className="snippet">
          <pre>{fix.ShowGrant.sql}</pre>
          <button type="button" className="ghost" onClick={() => copy(fix.ShowGrant.sql)}>
            Copy
          </button>
        </div>
      )}
      <div className="actions">
        {typeof fix === "object" && "SwitchSchema" in fix && connectionId && (
          <button type="button" disabled={busy} onClick={() => switchSchema(fix.SwitchSchema)}>
            Switch to {fix.SwitchSchema}
          </button>
        )}
        {fix === "SignIn" && onSignIn && (
          <button type="button" onClick={onSignIn}>
            Sign in again
          </button>
        )}
        {fix === "Retry" && onRetry && (
          <button type="button" onClick={onRetry}>
            Try again
          </button>
        )}
        {fix === "Rollback" && onRollback && (
          <button type="button" onClick={onRollback}>
            Roll back
          </button>
        )}
      </div>
      {note && <p className="muted">{note}</p>}
    </div>
  );
}
