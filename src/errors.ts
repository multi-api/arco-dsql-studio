import { explainErrorOffline } from "./api";
import type { CoreError, Explanation, SavedConnection } from "./types";

/** A one-line description of any error, for logs and fallbacks. */
export function errorText(err: unknown): string {
  if (typeof err === "string") return err;
  if (err && typeof err === "object") {
    const e = err as Exclude<CoreError, "Cancelled">;
    if ("Aws" in e) return e.Aws.message;
    if ("Db" in e) return e.Db.sqlstate ? `${e.Db.message} (${e.Db.sqlstate})` : e.Db.message;
    if ("Io" in e) return e.Io;
    if ("Config" in e) return e.Config;
  }
  if (err === "Cancelled") return "Cancelled";
  return String(err);
}

/** The core's explanation of an error with no open connection; never rejects. */
export async function explainOffline(saved: SavedConnection | null, err: unknown): Promise<Explanation> {
  try {
    return await explainErrorOffline(saved, err as CoreError);
  } catch {
    return { title: "Something went wrong", body: errorText(err), fix: "None" };
  }
}
