/** The exact statements a save will run, shown before anything is sent. Production needs an explicit confirmation. */
export default function SaveDialog({
  batches,
  production,
  busy,
  onApply,
  onCancel,
}: {
  batches: string[][];
  production: boolean;
  busy: boolean;
  onApply: () => void;
  onCancel: () => void;
}) {
  const count = batches.reduce((n, b) => n + b.length, 0);
  return (
    <div className="modal-backdrop">
      <div
        className={`modal${production ? " danger" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="save-title"
      >
        <h2 id="save-title">{production ? "Save to production?" : "Save changes"}</h2>
        <p>
          {count} {count === 1 ? "statement" : "statements"}, one row each
          {production && (
            <>
              {" "}
              on a connection marked as <strong>production</strong>
            </>
          )}
          :
        </p>
        {batches.length > 1 && (
          <p className="notice">
            DSQL changes at most 3,000 rows per transaction, so this runs as {batches.length} transactions. If one fails,
            the ones before it stay saved.
          </p>
        )}
        <pre className="statements">{batches.flat().join(";\n") + ";"}</pre>
        <div className="actions">
          <button type="button" className="ghost" onClick={onCancel} autoFocus={production}>
            Cancel
          </button>
          {production ? (
            <button type="button" className="danger" onClick={onApply} disabled={busy}>
              I understand, apply to production
            </button>
          ) : (
            <button type="button" onClick={onApply} disabled={busy}>
              Apply
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
