// Turning grid rows into text for the clipboard or a file. `null` is SQL NULL and stays distinct from "" wherever
// the format allows it.

type Rows = (string | null)[][];

const ident = (s: string) => `"${s.replace(/"/g, '""')}"`;
const literal = (v: string | null) => (v === null ? "NULL" : `'${v.replace(/'/g, "''")}'`);

/** RFC 4180: CRLF line ends, fields with a comma, quote or line break quoted. NULL and "" are both empty. */
export function toCsv(columns: string[], rows: Rows): string {
  const field = (v: string | null) => {
    if (v === null) return "";
    return /[",\r\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v;
  };
  return [columns, ...rows].map((r) => r.map(field).join(",") + "\r\n").join("");
}

/** Tab-separated, for pasting into a spreadsheet. Tabs and line breaks in values become spaces; NULL is \N. */
export function toTsv(columns: string[], rows: Rows): string {
  const field = (v: string | null) => (v === null ? "\\N" : v.replace(/[\t\r\n]+/g, " "));
  return [columns, ...rows].map((r) => r.map(field).join("\t") + "\n").join("");
}

/** An array of objects keyed by column, values as text or null. */
export function toJson(columns: string[], rows: Rows): string {
  return JSON.stringify(
    rows.map((r) => Object.fromEntries(columns.map((c, i) => [c, r[i] ?? null]))),
    null,
    2,
  );
}

/** One INSERT per row, names and values quoted, ready to run elsewhere. */
export function toInserts(schema: string, table: string, columns: string[], rows: Rows): string {
  const target = `${ident(schema)}.${ident(table)} (${columns.map(ident).join(", ")})`;
  return rows.map((r) => `INSERT INTO ${target} VALUES (${r.map(literal).join(", ")});`).join("\n");
}
