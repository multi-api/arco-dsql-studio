import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api");

import * as api from "../api";
import App from "../App";

const m = vi.mocked(api);

describe("App", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    m.listConnections.mockResolvedValue([]);
  });

  it("renders the app name and the profiles", async () => {
    m.listProfiles.mockResolvedValue([
      {
        name: "staging",
        kind: "Sso",
        sso_session: "corp",
        sso_start_url: "https://corp.awsapps.com/start",
        sso_region: "us-east-1",
        sso_account_id: "111122223333",
        sso_role_name: "Developer",
        region: "us-east-1",
      },
    ]);
    render(<App />);
    expect(screen.getByRole("heading", { name: "Arco DSQL Studio" })).toBeInTheDocument();
    expect(await screen.findByText("staging")).toBeInTheDocument();
    expect(screen.getByText("SSO")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in" })).toBeInTheDocument();
  });

  it("offers AWS setup when there are no profiles", async () => {
    m.listProfiles.mockResolvedValue([]);
    render(<App />);
    expect(await screen.findByText(/no aws profiles/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /set up aws access/i })).toBeInTheDocument();
  });
});
