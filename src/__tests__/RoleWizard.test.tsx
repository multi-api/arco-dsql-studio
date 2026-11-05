import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api");

import * as api from "../api";
import RoleWizard from "../components/RoleWizard";
import type { RolePlan, SavedConnection, TableInfo } from "../types";

const m = vi.mocked(api);
const ARN = "arn:aws:iam::111122223333:role/aws-reserved/sso.amazonaws.com/Developer_abc";
const saved: SavedConnection = {
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
const table = (name: string): TableInfo => ({
  schema: "app",
  name,
  kind: "Table",
  owner: "app_owner",
  can_select: false,
  can_insert: false,
  can_update: false,
  can_delete: false,
});
const createPlan: RolePlan = {
  admin_sql: [
    'CREATE ROLE "readonly" WITH LOGIN',
    `AWS IAM GRANT "readonly" TO '${ARN}'`,
    'GRANT USAGE ON SCHEMA "app" TO "readonly"',
    'ALTER ROLE "readonly" SET search_path = "app"',
  ],
  owner_sql: ['GRANT SELECT ON "app"."users" TO "readonly"'],
  owner_tinker: `DB::statement('GRANT SELECT ON "app"."users" TO "readonly"');`,
};
const removePlan: RolePlan = {
  admin_sql: [`AWS IAM REVOKE "readonly" FROM '${ARN}'`, 'REVOKE USAGE ON SCHEMA "app" FROM "readonly"', 'DROP ROLE "readonly"'],
  owner_sql: ['REVOKE ALL ON "app"."users" FROM "readonly"'],
  owner_tinker: `DB::statement('REVOKE ALL ON "app"."users" FROM "readonly"');`,
};
const ok = { outcomes: [{ Command: { tag: "CREATE ROLE", rows_affected: 0 } }], duration_ms: 3 };

function open(mode: "create" | "remove" = "create") {
  render(<RoleWizard mode={mode} connectionId="conn-1" saved={saved} schema="app" onSaveConnection={() => {}} />);
}

async function toReview() {
  fireEvent.click(await screen.findByRole("button", { name: /continue/i }));
  expect(await screen.findByLabelText(/iam role arn/i)).toHaveValue(ARN);
  fireEvent.click(screen.getByRole("button", { name: /continue/i }));
  fireEvent.click(await screen.findByRole("button", { name: /review/i }));
}

describe("RoleWizard", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    m.iamRoleForProfile.mockResolvedValue({ arn: ARN, guessed: false });
    m.listTables.mockResolvedValue([table("users")]);
    m.roleCreatePlan.mockResolvedValue(createPlan);
    m.roleRemovePlan.mockResolvedValue(removePlan);
    m.roleRunAdmin.mockResolvedValue(ok);
    m.roleAdminCanTakeOver.mockResolvedValue(false);
    m.roleConsoleFunctionName.mockResolvedValue("app-console");
    m.roleTakeoverPlan.mockImplementation(async (owner, sql) => [`GRANT "${owner}" TO admin`, ...sql, `REVOKE "${owner}" FROM admin`]);
    m.roleMappings.mockResolvedValue([["readonly", ARN]]);
  });

  it("wizard_shows_sql_before_running", async () => {
    open();
    await toReview();
    expect(await screen.findByText('CREATE ROLE "readonly" WITH LOGIN', { exact: false })).toBeInTheDocument();
    expect(screen.getByText('GRANT SELECT ON "app"."users" TO "readonly"', { exact: false })).toBeInTheDocument();
    expect(m.roleCreatePlan).toHaveBeenCalledWith("readonly", ARN, "app", ["users"], "ReadOnly");
    expect(m.roleRunAdmin).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: /run the admin steps/i }));
    await waitFor(() => expect(m.roleRunAdmin).toHaveBeenCalledWith("conn-1", createPlan.admin_sql));
    expect(await screen.findByText(/only the tables' owner/i)).toBeInTheDocument();
  });

  it("admin_takeover_hidden_unless_supported", async () => {
    open();
    await toReview();
    fireEvent.click(await screen.findByRole("button", { name: /run the admin steps/i }));
    await screen.findByText(/only the tables' owner/i);
    expect(m.roleAdminCanTakeOver).toHaveBeenCalledWith("conn-1", "app_owner");
    expect(screen.queryByText(/let admin do it/i)).not.toBeInTheDocument();
    expect(screen.getByText(/run it for me/i)).toBeInTheDocument();
  });

  it("offers the admin takeover with consent when admin holds ADMIN OPTION", async () => {
    m.roleAdminCanTakeOver.mockResolvedValue(true);
    open();
    await toReview();
    fireEvent.click(await screen.findByRole("button", { name: /run the admin steps/i }));
    fireEvent.click(await screen.findByLabelText(/let admin do it/i));
    const run = screen.getByRole("button", { name: /grant as admin/i });
    expect(run).toBeDisabled();
    fireEvent.click(screen.getByLabelText(/i agree/i));
    await waitFor(() => expect(run).toBeEnabled());
    fireEvent.click(run);
    await waitFor(() =>
      expect(m.roleRunAdmin).toHaveBeenLastCalledWith("conn-1", [
        'GRANT "app_owner" TO admin',
        ...createPlan.owner_sql,
        'REVOKE "app_owner" FROM admin',
      ]),
    );
  });

  it("remove_flow_orders_revokes_first", async () => {
    open("remove");
    fireEvent.change(await screen.findByLabelText(/role to remove/i), { target: { value: "readonly" } });
    fireEvent.click(screen.getByRole("button", { name: /review/i }));
    expect(await screen.findByText('REVOKE ALL ON "app"."users" FROM "readonly"', { exact: false })).toBeInTheDocument();
    expect(m.roleRemovePlan).toHaveBeenCalledWith("readonly", ARN, "app", ["users"]);

    const drop = screen.getByRole("button", { name: /remove the role/i });
    expect(drop).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: /the owner ran it/i }));
    expect(drop).toBeEnabled();
    fireEvent.click(drop);
    await waitFor(() => expect(m.roleRunAdmin).toHaveBeenCalledWith("conn-1", removePlan.admin_sql));
    const sent = m.roleRunAdmin.mock.calls[0][1];
    expect(sent[sent.length - 1]).toBe('DROP ROLE "readonly"');
  });
});
