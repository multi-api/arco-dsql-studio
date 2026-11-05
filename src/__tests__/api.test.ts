import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import * as api from "../api";

describe("api", () => {
  beforeEach(() => invoke.mockReset().mockResolvedValue(undefined));

  it("passes arguments by Tauri's camelCase names", async () => {
    await api.ssoStartLogin("https://example.awsapps.com/start", "us-east-1");
    expect(invoke).toHaveBeenLastCalledWith("sso_start_login", {
      startUrl: "https://example.awsapps.com/start",
      ssoRegion: "us-east-1",
    });

    await api.roleCreatePlan("readonly", "arn:aws:iam::111122223333:role/r", "app", ["users"], "ReadOnly");
    expect(invoke).toHaveBeenLastCalledWith("role_create_plan", {
      role: "readonly",
      iamRoleArn: "arn:aws:iam::111122223333:role/r",
      schema: "app",
      tables: ["users"],
      access: "ReadOnly",
    });

    await api.roleRunOwnerViaConsole("p", "us-east-1", "app-staging-console", "echo 1;");
    expect(invoke).toHaveBeenLastCalledWith("role_run_owner_via_console", {
      profile: "p",
      region: "us-east-1",
      function: "app-staging-console",
      php: "echo 1;",
    });
  });

  it("defaults optional arguments to null", async () => {
    await api.listClusters("p");
    expect(invoke).toHaveBeenLastCalledWith("list_clusters", { profile: "p", regions: null });
    await api.callerIdentity("p");
    expect(invoke).toHaveBeenLastCalledWith("caller_identity", { profile: "p", region: null });
  });

  it("returns what the command returns and rejects with its CoreError", async () => {
    invoke.mockResolvedValueOnce({ id: "conn-1", current_user: "admin", schema: "app" });
    await expect(
      api.openConnection({
        id: 1,
        name: "staging",
        profile: "p",
        region: "us-east-1",
        cluster_id: "c",
        host: "c.dsql.us-east-1.on.aws",
        role: "admin",
        schema: null,
        production: false,
        read_only: false,
      }),
    ).resolves.toEqual({ id: "conn-1", current_user: "admin", schema: "app" });

    invoke.mockRejectedValueOnce({ Config: "connection conn-9 is not open" });
    await expect(api.runSql("conn-9", "select 1")).rejects.toEqual({
      Config: "connection conn-9 is not open",
    });
  });
});
