import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { scratchLocation } from "../locations";
import { emptyContent } from "../request/content";
import { ConnectBar } from "./ConnectBar";
import { useWsStore, type WsTab } from "./wsStore";

vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));
vi.mock("../environments/usePreviewTitle", () => ({ usePreviewTitle: () => undefined }));
vi.mock("./api");

const loc = scratchLocation("s1");
const content = { ...emptyContent(), kind: "ws" as const, url: "wss://x/ws" };
const connect = vi.fn();
const disconnect = vi.fn();
const tab = (over: Partial<WsTab>): WsTab => ({
  status: "idle",
  opened: null,
  log: [],
  error: null,
  closed: null,
  composer: { format: "text", text: "" },
  restored: false,
  ...over,
});
const setup = (over: { dirty?: boolean; onSave?: () => void } = {}) =>
  render(
    <ConnectBar
      tabId="t"
      location={loc}
      content={content}
      variables={[]}
      dirty={over.dirty ?? false}
      onUrl={vi.fn()}
      onSave={over.onSave ?? vi.fn()}
    />,
  );

describe("ConnectBar", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useWsStore.getState().reset();
    useWsStore.setState({ connect, disconnect });
  });

  it("connects with the current content", async () => {
    setup();
    expect(screen.getByLabelText("Connection status")).toHaveTextContent("Idle");
    await userEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(connect).toHaveBeenCalledWith("t", loc, content);
  });

  it("connects on Mod-Enter in the URL field", async () => {
    setup();
    await userEvent.type(screen.getByLabelText("URL"), "{Control>}{Enter}{/Control}");
    expect(connect).toHaveBeenCalledTimes(1);
  });

  it("shows Disconnect, the pill and the protocol while open", async () => {
    useWsStore.setState({
      byTab: { t: tab({ status: "open", opened: { connection_id: "t", url: "wss://x/ws", protocol: "chat.v2", status: 101, headers: [] } }) },
    });
    setup();
    expect(screen.getByLabelText("Connection status")).toHaveTextContent("Open");
    expect(screen.getByText("chat.v2")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Connect" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    expect(disconnect).toHaveBeenCalledWith("t");
  });

  it("offers Cancel while connecting", async () => {
    useWsStore.setState({ byTab: { t: tab({ status: "connecting" }) } });
    setup();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(disconnect).toHaveBeenCalledWith("t");
  });

  it("saves only when dirty", async () => {
    const onSave = vi.fn();
    setup({ dirty: true, onSave });
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onSave).toHaveBeenCalled();
  });
});
