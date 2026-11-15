import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { scratchLocation } from "../locations";
import { emptyContent } from "../request/content";
import { useWsStore, type WsTab } from "../ws/wsStore";
import { SubscriptionBar } from "./SubscriptionBar";

vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));
vi.mock("../environments/usePreviewTitle", () => ({ usePreviewTitle: () => undefined }));
vi.mock("../ws/api");

const loc = scratchLocation("s1");
const content = { ...emptyContent(), kind: "graphql" as const, url: "https://x/graphql" };
const subscribe = vi.fn();
const unsubscribe = vi.fn();
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
const setup = (over: { dirty?: boolean; onSave?: () => void; url?: string } = {}) =>
  render(
    <SubscriptionBar
      tabId="t"
      location={loc}
      content={{ ...content, subscription: { url: over.url ?? "", init_payload: "" } }}
      variables={[]}
      dirty={over.dirty ?? false}
      onUrl={vi.fn()}
      onSave={over.onSave ?? vi.fn()}
    />,
  );

describe("SubscriptionBar", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useWsStore.getState().reset();
    useWsStore.setState({ subscribe, unsubscribe });
  });

  it("starts with the current content and shows the derived URL", async () => {
    setup();
    expect(screen.getByLabelText("Subscription status")).toHaveTextContent("Not started");
    expect(screen.getByLabelText("Subscription endpoint")).toHaveTextContent("wss://x/graphql");
    await userEvent.click(screen.getByRole("button", { name: "Start" }));
    expect(subscribe).toHaveBeenCalledWith("t", loc, expect.objectContaining({ url: "https://x/graphql" }));
  });

  it("shows the override URL", () => {
    setup({ url: "ws://other/ws" });
    expect(screen.getByLabelText("Subscription endpoint")).toHaveTextContent("ws://other/ws");
  });

  it("stops a live subscription", async () => {
    useWsStore.setState({ byTab: { t: tab({ status: "open" }) } });
    setup();
    expect(screen.getByLabelText("Subscription status")).toHaveTextContent("Subscribed");
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(unsubscribe).toHaveBeenCalledWith("t");
  });

  it("Mod-Enter toggles Start and Stop, and is ignored while stopping", async () => {
    setup();
    await userEvent.type(screen.getByLabelText("URL"), "{Control>}{Enter}{/Control}");
    expect(subscribe).toHaveBeenCalledTimes(1);
  });

  it("disables Stop while closing", () => {
    useWsStore.setState({ byTab: { t: tab({ status: "closing" }) } });
    setup();
    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
    expect(screen.getByLabelText("Subscription status")).toHaveTextContent("Stopping");
  });

  it("saves when dirty", async () => {
    const onSave = vi.fn();
    setup({ dirty: true, onSave });
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onSave).toHaveBeenCalled();
  });
});
