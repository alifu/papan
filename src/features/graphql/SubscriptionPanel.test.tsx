import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WsMessage } from "../../ipc/bindings";
import { scratchLocation } from "../locations";
import * as wsApi from "../ws/api";
import { useWsStore, type WsTab } from "../ws/wsStore";
import { SubscriptionPanel } from "./SubscriptionPanel";

vi.mock("../ws/api");

const loc = scratchLocation("s1");
const frame = (seq: number, text: string): WsMessage => ({ seq, at_ms: 0, direction: "received", frame: "text", text, size: text.length, truncated: false });
const tab = (over: Partial<WsTab>): WsTab => ({
  status: "open",
  opened: null,
  log: [],
  error: null,
  closed: null,
  composer: { format: "text", text: "" },
  restored: true,
  ...over,
});

describe("SubscriptionPanel", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useWsStore.getState().reset();
    vi.mocked(wsApi.wsLog).mockResolvedValue([]);
  });

  it("shows results, not protocol frames, and toggles to the raw frames", async () => {
    const log = [frame(1, '{"type":"connection_ack"}'), frame(2, '{"id":"1","type":"next","payload":{"data":{"n":1}}}')];
    useWsStore.setState({ byTab: { t: tab({ log }) } });
    render(<SubscriptionPanel tabId="t" location={loc} />);
    expect(screen.getByRole("status")).toHaveTextContent("1 result");
    await userEvent.click(screen.getByRole("button", { name: "Protocol frames" }));
    expect(screen.getByLabelText("Message log")).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
  });

  it("shows a start error", () => {
    useWsStore.setState({ byTab: { t: tab({ error: { code: "network", message: "Unauthorized (4401)", detail: null } }) } });
    render(<SubscriptionPanel tabId="t" location={loc} />);
    expect(screen.getByRole("alert")).toHaveTextContent("Unauthorized (4401)");
  });

  it("Clear empties the backend log", async () => {
    vi.mocked(wsApi.wsClearLog).mockResolvedValue(null);
    useWsStore.setState({ byTab: { t: tab({ log: [frame(1, '{"type":"next","payload":{}}')] }) }, connByTab: { t: "c1" } });
    render(<SubscriptionPanel tabId="t" location={loc} />);
    await userEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(wsApi.wsClearLog).toHaveBeenCalledWith(expect.any(String));
  });
});
