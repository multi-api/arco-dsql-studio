import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api");

import * as api from "../api";
import Connect from "../screens/Connect";

const m = vi.mocked(api);
const cluster = { identifier: "abc123", region: "us-east-1", host: "abc123.dsql.us-east-1.on.aws", tags: {} };

describe("Connect", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    m.openConnection.mockResolvedValue({ id: "conn-1", current_user: "admin", schema: null });
    m.listSchemas.mockResolvedValue([
      { name: "shop_staging", owner: "admin" },
      { name: "shop_production", owner: "admin" },
    ]);
    m.setSchema.mockResolvedValue(undefined);
    m.looksLikeProduction.mockImplementation(async (schema) => schema.endsWith("_production"));
    m.saveConnection.mockResolvedValue(7);
    m.getSetting.mockResolvedValue(null);
    m.setSetting.mockResolvedValue(undefined);
  });

  it("connect_asks_schema_when_ambiguous", async () => {
    const onConnected = vi.fn();
    render(<Connect profile="staging" cluster={cluster} onConnected={onConnected} onBack={() => {}} />);

    fireEvent.click(screen.getByRole("button", { name: /^connect$/i }));
    expect(await screen.findByText(/pick the schema/i)).toBeInTheDocument();
    expect(m.openConnection).toHaveBeenCalledWith(
      expect.objectContaining({ role: "admin", schema: null, host: cluster.host, read_only: false }),
    );

    fireEvent.click(screen.getByLabelText("shop_production"));
    await waitFor(() => expect(m.setSchema).toHaveBeenCalledWith("conn-1", "shop_production"));
    await waitFor(() => expect(screen.getByLabelText(/this is production/i)).toBeChecked());

    fireEvent.click(screen.getByRole("button", { name: /save and open/i }));
    await waitFor(() => expect(onConnected).toHaveBeenCalled());
    expect(m.saveConnection).toHaveBeenCalledWith(
      expect.objectContaining({ schema: "shop_production", production: true, cluster_id: "abc123" }),
    );
    expect(m.setSetting).toHaveBeenCalledWith("last_regions", JSON.stringify(["us-east-1"]));
  });

  it("sends the custom role name", async () => {
    m.openConnection.mockResolvedValue({ id: "conn-2", current_user: "readonly", schema: "shop_staging" });
    m.looksLikeProduction.mockResolvedValue(false);
    render(<Connect profile="staging" cluster={cluster} onConnected={() => {}} onBack={() => {}} />);
    fireEvent.click(screen.getByLabelText(/a custom role/i));
    fireEvent.change(screen.getByLabelText(/role name/i), { target: { value: "readonly" } });
    fireEvent.click(screen.getByRole("button", { name: /^connect$/i }));
    await waitFor(() =>
      expect(m.openConnection).toHaveBeenCalledWith(expect.objectContaining({ role: "readonly" })),
    );
    expect(await screen.findByText(/connected as/i)).toHaveTextContent("shop_staging");
    expect(m.listSchemas).not.toHaveBeenCalled();
  });
});
