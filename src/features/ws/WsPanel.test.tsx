import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RequestContent, WsMessage } from "../../ipc/bindings";
import { scratchLocation } from "../locations";
import { emptyContent } from "../request/content";
import * as api from "./api";
import { WsPanel } from "./WsPanel";
import { useWsStore } from "./wsStore";

vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));
vi.mock("./api");

const loc = scratchLocation("s1");
const content: RequestContent = { ...emptyContent(), kind: "ws", url: "wss://x/ws" };
const CID = "11111111-1111-4111-8111-111111111111";
const opened = { connection_id: CID, url: "wss://x/ws", protocol: null, status: 101, headers: [] };
const sent: WsMessage = { seq: 1, at_ms: 0, direction: "sent", frame: "text", text: "hi", size: 2, truncated: false };

const setup = (onChange = vi.fn()) => {
  render(<WsPanel tabId="t" location={loc} content={content} variables={[]} onChange={onChange} />);
  return onChange;
};

describe("WsPanel", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.spyOn(crypto, "randomUUID").mockReturnValue(CID);
    useWsStore.getState().reset();
    vi.mocked(api.wsLog).mockResolvedValue([]);
  });

  it("restores the backend log on mount", async () => {
    vi.mocked(api.wsLog).mockResolvedValue([sent]);
    setup();
    expect(await screen.findByText("hi")).toBeVisible();
  });

  it("sends the composed message through the store", async () => {
    vi.mocked(api.wsSend).mockResolvedValue(sent);
    setup();
    await act(async () => useWsStore.getState().apply({ connection_id: CID, kind: { type: "opened", ...opened } }));
    await userEvent.type(screen.getByLabelText("Message"), "hi");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(api.wsSend).toHaveBeenCalledWith(CID, loc, { frame: "text", text: "hi" });
    expect(await screen.findByLabelText("Sent")).toBeVisible();
  });

  it("keeps the composer text when the panel is re-mounted", async () => {
    const view = render(<WsPanel tabId="t" location={loc} content={content} variables={[]} onChange={vi.fn()} />);
    await userEvent.type(screen.getByLabelText("Message"), "draft");
    view.unmount();
    render(<WsPanel tabId="t" location={loc} content={content} variables={[]} onChange={vi.fn()} />);
    expect(screen.getByLabelText("Message")).toHaveValue("draft");
  });

  it("saves the composer as a template through onChange", async () => {
    const onChange = setup();
    await userEvent.type(screen.getByLabelText("Message"), "hello");
    await userEvent.type(screen.getByLabelText("Template name"), "Greet");
    await userEvent.click(screen.getByRole("button", { name: "Save as template" }));
    const update = onChange.mock.calls[0]?.[0] as (c: RequestContent) => RequestContent;
    expect(update(content).ws?.messages).toEqual([{ name: "Greet", format: "text", text: "hello" }]);
  });

  it("explains how the connection ended", async () => {
    setup();
    await act(async () =>
      useWsStore.getState().apply({
        connection_id: CID,
        kind: { type: "closed", code: 1001, reason: "going away", by: "server", error: null },
      }),
    );
    expect(screen.getByText(/Closed by the server \(1001\): going away/)).toBeVisible();
  });

  it("shows a connection error", () => {
    useWsStore.setState({
      byTab: {
        t: {
          status: "closed",
          opened: null,
          log: [],
          error: { code: "network", message: "Connection refused", detail: null },
          closed: null,
          composer: { format: "text", text: "" },
          restored: true,
        },
      },
    });
    setup();
    expect(screen.getByRole("alert")).toHaveTextContent("Connection refused");
  });
});
