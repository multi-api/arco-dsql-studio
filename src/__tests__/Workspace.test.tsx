import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api");
// CodeMirror needs a real layout engine; a textarea stands in for it.
vi.mock("../components/SqlEditor", () => ({
  default: ({ value, onChange }: { value: string; onChange: (v: string) => void }) => (
    <textarea aria-label="SQL" value={value} onChange={(e) => onChange(e.target.value)} />
  ),
}));

import * as api from "../api";
import Workspace from "../screens/Workspace";
import type { ClassifiedStatement, SavedConnection, SqlCheck } from "../types";

const m = vi.mocked(api);
const handle = { id: "conn-1", current_user: "admin", schema: "app" };
const base: SavedConnection = {
  id: 1,
  name: "shop · app",
  profile: "p",
  region: "us-east-1",
  cluster_id: "c",
  host: "c.dsql.us-east-1.on.aws",
  role: "admin",
  schema: "app",
  production: false,
  read_only: false,
};

function check(statement: Partial<ClassifiedStatement>, flags: Partial<SqlCheck> = {}): SqlCheck {
  return {
    classified: { statements: [{ sql: "", kind: "Read", findings: [], ...statement }] },
    blocked: false,
    needs_confirmation: false,
    ...flags,
  };
}

function open(saved: Partial<SavedConnection> = {}) {
  render(<Workspace handle={handle} saved={{ ...base, ...saved }} onDisconnect={() => {}} />);
}

function typeAndRun(sql: string) {
  m.statementAt.mockResolvedValue(sql);
  fireEvent.change(screen.getByLabelText("SQL"), { target: { value: sql } });
  fireEvent.click(screen.getByRole("button", { name: /^run$/i }));
}

describe("Workspace", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    m.listSchemas.mockResolvedValue([{ name: "app", owner: "admin" }]);
    m.listTables.mockResolvedValue([]);
    m.searchHistory.mockResolvedValue([]);
    m.getSetting.mockResolvedValue(null);
    m.runSql.mockResolvedValue({ outcomes: [{ Command: { tag: "TRUNCATE", rows_affected: 0 } }], duration_ms: 5 });
  });

  it("run_warns_on_truncate_and_can_proceed", async () => {
    m.checkSql.mockResolvedValue(
      check({
        sql: "truncate t",
        kind: "Schema",
        findings: [
          {
            rule_id: "truncate",
            message: "DSQL does not support TRUNCATE.",
            alternative: "Use DELETE FROM <table>.",
            docs_url: null,
          },
        ],
      }),
    );
    open();
    typeAndRun("truncate t");

    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("DSQL does not support TRUNCATE.");
    expect(dialog).toHaveTextContent("Use DELETE FROM <table>.");
    expect(m.runSql).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: /run anyway/i }));
    await waitFor(() => expect(m.runSql).toHaveBeenCalledWith("conn-1", "truncate t"));
  });

  it("production_delete_requires_confirm", async () => {
    m.checkSql.mockResolvedValue(check({ sql: "delete from t", kind: "Write" }, { needs_confirmation: true }));
    open({ production: true });
    expect(screen.getByText("PRODUCTION")).toBeInTheDocument();
    typeAndRun("delete from t");

    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(/production/i);
    expect(dialog).toHaveTextContent("delete from t");
    expect(m.runSql).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: /cancel/i }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(m.runSql).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: /^run$/i }));
    fireEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: /run on production/i }));
    await waitFor(() => expect(m.runSql).toHaveBeenCalledWith("conn-1", "delete from t"));
  });

  it("read_only_blocks_insert", async () => {
    m.checkSql.mockResolvedValue(check({ sql: "insert into t values (1)", kind: "Write" }, { blocked: true }));
    open({ read_only: true });
    expect(screen.getByText(/read-only/i)).toBeInTheDocument();
    typeAndRun("insert into t values (1)");

    expect(await screen.findByText(/this connection is read-only/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /run anyway/i })).not.toBeInTheDocument();
    expect(m.runSql).not.toHaveBeenCalled();
  });

  it("shows results and explains errors", async () => {
    m.checkSql.mockResolvedValue(check({ sql: "select * from users" }));
    const err = { Db: { sqlstate: "42501", message: "permission denied for table users", detail: null, hint: null } };
    m.runSql.mockResolvedValue({ outcomes: [{ Error: err }], duration_ms: 3 });
    m.explainError.mockResolvedValue({
      title: "Your role cannot read users",
      body: "users belongs to app.",
      fix: { ShowGrant: { table: "users", owner: "app", sql: 'GRANT SELECT ON "app"."users" TO "admin";' } },
    });
    open();
    typeAndRun("select * from users");
    expect(await screen.findByRole("alert")).toHaveTextContent("Your role cannot read users");
    expect(m.explainError).toHaveBeenCalledWith("conn-1", err);
    expect(screen.getByText('GRANT SELECT ON "app"."users" TO "admin";')).toBeInTheDocument();
  });

  it("history_click_opens_tab", async () => {
    m.searchHistory.mockResolvedValue([
      {
        id: 3,
        connection_id: 1,
        sql: "select count(*) from orders",
        started_at: "2026-11-05T10:00:00Z",
        duration_ms: 12,
        rows: 1,
        error: null,
      },
    ]);
    open();
    fireEvent.click(screen.getByRole("button", { name: /history/i }));
    fireEvent.click(await screen.findByText("select count(*) from orders"));

    expect(screen.getAllByRole("tab")).toHaveLength(2);
    expect(screen.getByRole("tab", { selected: true })).toHaveTextContent(/select count/i);
    expect(screen.getByLabelText("SQL")).toHaveValue("select count(*) from orders");
  });
});
