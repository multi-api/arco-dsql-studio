import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api");

import * as api from "../api";
import ExplainBox from "../components/ExplainBox";

const m = vi.mocked(api);

describe("ExplainBox", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    m.setSchema.mockResolvedValue(undefined);
  });

  it("explain_box_switch_schema_calls_set_schema", async () => {
    const onSchemaChanged = vi.fn();
    render(
      <ExplainBox
        explanation={{
          title: "users is in another schema",
          body: "Your session looks in public.",
          fix: { SwitchSchema: "app_staging" },
        }}
        connectionId="conn-1"
        onSchemaChanged={onSchemaChanged}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("users is in another schema");
    fireEvent.click(screen.getByRole("button", { name: /switch to app_staging/i }));
    await waitFor(() => expect(m.setSchema).toHaveBeenCalledWith("conn-1", "app_staging"));
    expect(onSchemaChanged).toHaveBeenCalledWith("app_staging");
  });

  it("shows the grant to copy and the sign-in fix", () => {
    const onSignIn = vi.fn();
    const { rerender } = render(
      <ExplainBox
        explanation={{
          title: "Your role cannot read users",
          body: "Ask the owner.",
          fix: { ShowGrant: { table: "users", owner: "app", sql: 'GRANT SELECT ON "s"."users" TO "r";' } },
        }}
      />,
    );
    expect(screen.getByText('GRANT SELECT ON "s"."users" TO "r";')).toBeInTheDocument();
    rerender(<ExplainBox explanation={{ title: "Expired", body: "", fix: "SignIn" }} onSignIn={onSignIn} />);
    fireEvent.click(screen.getByRole("button", { name: /sign in again/i }));
    expect(onSignIn).toHaveBeenCalled();
  });
});
