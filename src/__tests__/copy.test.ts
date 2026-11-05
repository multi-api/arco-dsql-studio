import { describe, expect, it } from "vitest";

import { toCsv, toInserts, toJson, toTsv } from "../copy";

const columns = ["id", "name", "note"];
const rows = [
  ["1", "O'Brien", null],
  ["2", 'say "hi", ok', ""],
];

describe("copy", () => {
  it("copy_as_insert_quotes_values", () => {
    expect(toInserts("my app", "Order", columns, rows)).toBe(
      [
        `INSERT INTO "my app"."Order" ("id", "name", "note") VALUES ('1', 'O''Brien', NULL);`,
        `INSERT INTO "my app"."Order" ("id", "name", "note") VALUES ('2', 'say "hi", ok', '');`,
      ].join("\n"),
    );
  });

  it("csv quotes fields that need it and leaves NULL empty", () => {
    expect(toCsv(columns, rows)).toBe('id,name,note\r\n1,O\'Brien,\r\n2,"say ""hi"", ok",\r\n');
  });

  it("tsv and json keep NULL distinct from empty", () => {
    // Tabs and newlines inside a value become spaces; NULL is \N, as Postgres COPY writes it.
    expect(toTsv(["id", "name"], [["a\tb\nc", null]])).toBe("id\tname\na b c\t\\N\n");
    expect(JSON.parse(toJson(columns, rows))).toEqual([
      { id: "1", name: "O'Brien", note: null },
      { id: "2", name: 'say "hi", ok', note: "" },
    ]);
  });
});
