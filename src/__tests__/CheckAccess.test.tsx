import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api");

import * as api from "../api";
import CheckAccess from "../components/CheckAccess";

const m = vi.mocked(api);

describe("CheckAccess", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    m.listTables.mockResolvedValue([
      {
        schema: "app",
        name: "users",
        kind: "Table",
        owner: "app_owner",
        can_select: true,
        can_insert: true,
        can_update: true,
        can_delete: true,
      },
      {
        schema: "app",
        name: "Order Items",
        kind: "Table",
        owner: "app_owner",
        can_select: true,
        can_insert: false,
        can_update: false,
        can_delete: false,
      },
    ]);
  });

  it("check_access_lists_missing_grants", async () => {
    render(<CheckAccess connectionId="conn-1" schema="app" role="readonly" />);
    expect(await screen.findByText("Order Items")).toBeInTheDocument();
    expect(screen.getByText('GRANT INSERT, UPDATE, DELETE ON "app"."Order Items" TO "readonly";')).toBeInTheDocument();
    expect(screen.queryByText(/ON "app"."users"/)).not.toBeInTheDocument();
    expect(screen.getAllByText("app_owner")).toHaveLength(2);
  });
});
