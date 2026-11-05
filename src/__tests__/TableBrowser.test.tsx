import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api");
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("@tauri-apps/plugin-fs", () => ({ writeTextFile: vi.fn() }));

import * as api from "../api";
import TableBrowser from "../components/TableBrowser";
import type { ColumnInfo } from "../types";

const m = vi.mocked(api);

const col = (name: string, data_type: string, is_pk = false, ordinal = 1): ColumnInfo => ({
  name,
  data_type,
  nullable: !is_pk,
  default: null,
  is_pk,
  ordinal,
});

function open(props: { production?: boolean; readOnly?: boolean } = {}) {
  render(
    <TableBrowser
      connectionId="conn-1"
      schema="app"
      table="users"
      production={props.production ?? false}
      readOnly={props.readOnly ?? false}
    />,
  );
}

async function editCell(row: number, column: string, value: string | null) {
  const cell = screen.getAllByRole("row")[row + 1].querySelector(`[data-column="${column}"]`) as HTMLElement;
  fireEvent.doubleClick(cell);
  const input = await screen.findByLabelText(`Edit ${column}`);
  if (value === null) {
    fireEvent.click(screen.getByRole("button", { name: `Set ${column} to NULL` }));
  } else {
    fireEvent.change(input, { target: { value } });
    fireEvent.keyDown(input, { key: "Enter" });
  }
}

describe("TableBrowser", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    m.tableColumns.mockResolvedValue([col("id", "bigint", true, 1), col("name", "text", false, 2), col("note", "text", false, 3)]);
    m.tablePage.mockResolvedValue({
      result: {
        columns: ["id", "name", "note"],
        rows: [
          ["1", "a", null],
          ["2", "b", ""],
        ],
        truncated: false,
      },
      sql: 'SELECT * FROM "app"."users" LIMIT 200 OFFSET 0',
    });
    m.previewChanges.mockResolvedValue([[`UPDATE "app"."users" SET "name" = 'x'::text WHERE "id" = '1'::bigint`]]);
    m.applyChanges.mockResolvedValue({
      batches_applied: 1,
      batches_total: 1,
      rows_affected: 1,
      rows_missing: 0,
      retries: 0,
      error: null,
    });
  });

  it("save_dialog_lists_statements", async () => {
    open();
    expect(await screen.findByText("a")).toBeInTheDocument();
    await editCell(0, "name", "x");
    fireEvent.click(screen.getByRole("button", { name: /save 1 change/i }));

    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(`UPDATE "app"."users" SET "name" = 'x'::text WHERE "id" = '1'::bigint`);
    expect(dialog).toHaveTextContent(/1 statement/);
    expect(m.applyChanges).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: /^apply$/i }));
    await waitFor(() => expect(m.applyChanges).toHaveBeenCalled());
    expect(m.applyChanges.mock.calls[0][0]).toBe("conn-1");
    expect(m.applyChanges.mock.calls[0][1]).toMatchObject({
      schema: "app",
      table: "users",
      pk: ["id"],
      column_types: { id: "bigint", name: "text", note: "text" },
      updates: [{ pk_values: { id: "1" }, set: { name: "x" } }],
    });
  });

  it("production_save_needs_explicit_confirm", async () => {
    open({ production: true });
    await screen.findByText("a");
    await editCell(0, "name", "x");
    fireEvent.click(screen.getByRole("button", { name: /save 1 change/i }));

    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(/production/i);
    expect(within(dialog).queryByRole("button", { name: /^apply$/i })).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: /i understand, apply to production/i }));
    await waitFor(() => expect(m.applyChanges).toHaveBeenCalled());
  });

  it("no_pk_table_is_read_only", async () => {
    m.tableColumns.mockResolvedValue([col("name", "text"), col("note", "text", false, 2)]);
    m.tablePage.mockResolvedValue({
      result: { columns: ["name", "note"], rows: [["a", null]], truncated: false },
      sql: "",
    });
    open();
    expect(await screen.findByText(/no primary key/i)).toBeInTheDocument();
    fireEvent.doubleClick(screen.getByText("a"));
    expect(screen.queryByLabelText("Edit name")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /add row/i })).not.toBeInTheDocument();
  });

  it("null_toggle_differs_from_empty", async () => {
    open();
    await screen.findByText("a");
    await editCell(0, "name", "");
    await editCell(1, "note", null);
    fireEvent.click(screen.getByRole("button", { name: /save 2 changes/i }));
    await screen.findByRole("dialog");
    expect(m.previewChanges).toHaveBeenCalledWith(
      expect.objectContaining({
        updates: [
          { pk_values: { id: "1" }, set: { name: "" } },
          { pk_values: { id: "2" }, set: { note: null } },
        ],
      }),
    );
  });

  it("deletes selected rows and adds a row", async () => {
    open();
    await screen.findByText("a");
    fireEvent.click(screen.getByLabelText("Select row 2"));
    fireEvent.click(screen.getByRole("button", { name: /delete 1 row/i }));
    fireEvent.click(screen.getByRole("button", { name: /add row/i }));
    fireEvent.click(screen.getByRole("button", { name: /save 2 changes/i }));
    await screen.findByRole("dialog");
    expect(m.previewChanges).toHaveBeenCalledWith(
      expect.objectContaining({ deletes: [{ id: "2" }], inserts: [{}] }),
    );
  });

  it("read-only connections cannot edit", async () => {
    open({ readOnly: true });
    await screen.findByText("a");
    fireEvent.doubleClick(screen.getByText("a"));
    expect(screen.queryByLabelText("Edit name")).not.toBeInTheDocument();
  });
});
