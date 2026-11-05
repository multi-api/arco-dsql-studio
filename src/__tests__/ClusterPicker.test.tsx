import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api");

import * as api from "../api";
import ClusterPicker from "../screens/ClusterPicker";
import type { ProfileInfo } from "../types";

const m = vi.mocked(api);
const profile: ProfileInfo = {
  name: "staging",
  kind: "Sso",
  sso_session: "corp",
  sso_start_url: "https://corp.awsapps.com/start",
  sso_region: "us-east-1",
  sso_account_id: "111122223333",
  sso_role_name: "Developer",
  region: "us-east-1",
};

describe("ClusterPicker", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    m.callerIdentity.mockResolvedValue({
      account: "111122223333",
      arn: "arn:aws:sts::111122223333:assumed-role/Developer/me",
      user_id: "x",
    });
    m.getSetting.mockResolvedValue(null);
    m.listClusters.mockResolvedValue({
      clusters: [
        {
          identifier: "abc123",
          region: "us-east-1",
          status: "ACTIVE",
          endpoint: "abc123.dsql.us-east-1.on.aws",
          arn: "arn:aws:dsql:us-east-1:111122223333:cluster/abc123",
          tags: { Name: "shop" },
        },
      ],
      failed: [
        { region: "ap-east-1", error: { Aws: { kind: "NotAvailableInRegion", message: "invalid token" } } },
        { region: "eu-west-3", error: { Aws: { kind: "AccessDenied", message: "not authorized" } } },
      ],
    });
  });

  it("cluster_picker_skips_denied_regions_quietly", async () => {
    render(<ClusterPicker profile={profile} onPick={() => {}} onBack={() => {}} onSignIn={() => {}} />);
    expect(await screen.findByText("abc123")).toBeInTheDocument();
    expect(await screen.findByText(/signed in as/i)).toHaveTextContent("Developer");
    expect(screen.getByText(/signed in as/i)).toHaveTextContent("111122223333");
    expect(m.listClusters).toHaveBeenCalledWith("staging", null);
    expect(screen.queryByText(/ap-east-1/)).not.toBeInTheDocument();
    expect(screen.queryByText(/eu-west-3/)).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("scans only the remembered regions", async () => {
    m.getSetting.mockResolvedValue(JSON.stringify(["us-east-1"]));
    render(<ClusterPicker profile={profile} onPick={() => {}} onBack={() => {}} onSignIn={() => {}} />);
    expect(await screen.findByText("abc123")).toBeInTheDocument();
    expect(m.listClusters).toHaveBeenCalledWith("staging", ["us-east-1"]);
    expect(screen.getByRole("button", { name: /scan all regions/i })).toBeInTheDocument();
  });
});
