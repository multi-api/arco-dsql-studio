import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api");
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

import * as api from "../api";
import SetupAws from "../screens/SetupAws";

const m = vi.mocked(api);
const LINES = "[profile staging-developer]\nsso_session = corp\nsso_account_id = 111122223333\n";

describe("SetupAws", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    m.ssoStartLogin.mockResolvedValue({
      login_id: "login-1",
      verification_uri_complete: "https://device.sso.us-east-1.amazonaws.com/?user_code=ABCD-EFGH",
      user_code: "ABCD-EFGH",
      expires_in: 600,
    });
    m.ssoFinishLogin.mockResolvedValue(undefined);
    m.ssoListAccounts.mockResolvedValue([
      { account_id: "111122223333", account_name: "Staging", role_name: "Developer" },
    ]);
    m.previewSsoProfile.mockResolvedValue(LINES);
    m.appendSsoProfile.mockResolvedValue(undefined);
  });

  it("setup_wizard_shows_exact_lines_before_append", async () => {
    let approve = () => {};
    m.ssoFinishLogin.mockImplementation(() => new Promise<void>((resolve) => (approve = resolve)));
    const onDone = vi.fn();
    render(<SetupAws onDone={onDone} onCancel={() => {}} />);

    fireEvent.change(screen.getByLabelText(/access portal url/i), {
      target: { value: "https://corp.awsapps.com/start" },
    });
    fireEvent.change(screen.getByLabelText(/sso region/i), { target: { value: "us-east-1" } });
    fireEvent.click(screen.getByRole("button", { name: /sign in/i }));

    expect(await screen.findByText("ABCD-EFGH")).toBeInTheDocument();
    expect(m.ssoStartLogin).toHaveBeenCalledWith("https://corp.awsapps.com/start", "us-east-1");
    expect(m.ssoListAccounts).not.toHaveBeenCalled();
    approve();

    fireEvent.click(await screen.findByLabelText(/staging.*developer/i));
    fireEvent.click(screen.getByRole("button", { name: /continue/i }));

    expect(screen.getByLabelText(/profile name/i)).toHaveValue("staging-developer");
    fireEvent.click(screen.getByRole("button", { name: /preview/i }));

    const preview = await screen.findByTestId("profile-preview");
    expect(preview.textContent).toBe(LINES);
    expect(m.previewSsoProfile).toHaveBeenCalledWith({
      profile: "staging-developer",
      session: "corp",
      start_url: "https://corp.awsapps.com/start",
      sso_region: "us-east-1",
      account_id: "111122223333",
      role_name: "Developer",
      region: "us-east-1",
    });
    expect(m.appendSsoProfile).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: /add to/i }));
    await waitFor(() => expect(onDone).toHaveBeenCalledWith("staging-developer"));
    expect(m.appendSsoProfile).toHaveBeenCalledWith(
      expect.objectContaining({ profile: "staging-developer", session: "corp" }),
      "login-1",
    );
  });
});
