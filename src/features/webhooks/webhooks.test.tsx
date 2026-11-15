import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Capture, WebhookListener } from "../../ipc/bindings";
import { useTabsStore } from "../../shell/tabsStore";
import { CaptureDetail } from "./CaptureDetail";
import { ListenerEditor } from "./ListenerEditor";
import { DEFAULT_CONFIG } from "./schema";
import { WebhookTab } from "./WebhookTab";
import { WebhooksPanel } from "./WebhooksPanel";
import * as api from "./api";
import { useWebhookStore } from "./webhookStore";

vi.mock("./api");
vi.mock("../scratch/api", () => ({ listScratch: vi.fn().mockResolvedValue([]) }));
vi.mock("../response/clipboard", () => ({ copyText: vi.fn().mockResolvedValue(undefined) }));

const listener = (patch: Partial<WebhookListener> = {}): WebhookListener => ({
  config: { ...DEFAULT_CONFIG, id: "a", name: "Stripe" },
  running: false,
  url: null,
  captured: 0,
  unmatched: 0,
  ...patch,
});
const capture: Capture = {
  summary: { id: "c1", at_ms: 0, method: "POST", path: "/hooks", size: 9, content_type: "application/json" },
  query: "x=1",
  headers: [{ name: "X-Sig", value: "abc", enabled: true }],
  remote: "127.0.0.1:5",
  body_text: '{"ok":true}',
  body_base64: null,
  truncated: false,
};

beforeEach(() => {
  vi.clearAllMocks();
  useWebhookStore.setState({ listeners: [], captures: {} });
  useTabsStore.setState(useTabsStore.getInitialState(), true);
  vi.mocked(api.listCaptures).mockResolvedValue([]);
  vi.mocked(api.readCapture).mockResolvedValue(capture);
});

describe("WebhooksPanel", () => {
  it("shows the empty text and creates a listener, opening its tab", async () => {
    vi.mocked(api.saveWebhook).mockResolvedValue(listener());
    render(<WebhooksPanel />);
    expect(screen.getByText("No webhook listeners yet.")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "New Listener" }));
    await waitFor(() => expect(useTabsStore.getState().tabs[1]).toMatchObject({ kind: "webhook", webhookId: "a" }));
  });

  it("lists state, toggles start and stop, and opens the tab on click", async () => {
    useWebhookStore.setState({ listeners: [listener({ captured: 3 })] });
    vi.mocked(api.startWebhook).mockResolvedValue(listener({ running: true, url: "http://127.0.0.1:9/hooks" }));
    render(<WebhooksPanel />);
    expect(screen.getByRole("img", { name: "Stopped" })).toBeVisible();
    expect(screen.getByText("3")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Start Stripe" }));
    expect(await screen.findByRole("img", { name: "Running" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Stop Stripe" })).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: /^Stripe/ }));
    expect(useTabsStore.getState().tabs.some((t) => t.webhookId === "a")).toBe(true);
  });
});

describe("ListenerEditor", () => {
  it("shows validation errors and does not save", async () => {
    render(<ListenerEditor listener={listener()} />);
    await userEvent.clear(screen.getByLabelText("Path"));
    await userEvent.type(screen.getByLabelText("Path"), "nope");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Path must start with /.");
    expect(api.saveWebhook).not.toHaveBeenCalled();
  });

  it("saves a valid config", async () => {
    vi.mocked(api.saveWebhook).mockResolvedValue(listener());
    render(<ListenerEditor listener={listener()} />);
    await userEvent.clear(screen.getByLabelText("Port (0 for any free port)"));
    await userEvent.type(screen.getByLabelText("Port (0 for any free port)"), "8080");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.saveWebhook).toHaveBeenCalled());
    expect(vi.mocked(api.saveWebhook).mock.calls[0][0]).toMatchObject({ id: "a", port: 8080 });
  });

  it("warns when binding to all interfaces and edits response headers", async () => {
    render(<ListenerEditor listener={listener()} />);
    await userEvent.selectOptions(screen.getByLabelText("Bind"), "all_interfaces");
    expect(screen.getByRole("note")).toHaveTextContent("anyone on your network");

    await userEvent.click(screen.getByRole("button", { name: "Add header" }));
    expect(screen.getByLabelText("Header name 2")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Remove header 2" }));
    expect(screen.queryByLabelText("Header name 2")).toBeNull();
  });
});

describe("CaptureDetail", () => {
  it("shows query, headers and pretty JSON, and replays", async () => {
    vi.mocked(api.replayCapture).mockResolvedValue({
      item: { id: "s1", name: "POST /hooks", method: "POST", kind: "http" },
      warnings: [],
    });
    render(<CaptureDetail listener={listener({ running: true, url: "http://127.0.0.1:9/hooks" })} captureId="c1" />);

    expect(await screen.findByText("Query: x=1")).toBeVisible();
    expect(screen.getByText("X-Sig")).toBeVisible();
    expect(screen.getByLabelText("Body")).toHaveTextContent('"ok": true');

    await userEvent.click(screen.getByRole("button", { name: "Replay as new request" }));
    await waitFor(() => expect(api.replayCapture).toHaveBeenCalledWith("a", "c1"));
    await waitFor(() => expect(useTabsStore.getState().tabs[1].title).toBe("POST /hooks"));
  });

  it("shows base64 for binary bodies and a note for truncated ones", async () => {
    vi.mocked(api.readCapture).mockResolvedValueOnce({ ...capture, body_text: null, body_base64: "AQID" });
    const { unmount } = render(<CaptureDetail listener={listener()} captureId="c1" />);
    expect(await screen.findByLabelText("Body")).toHaveTextContent("AQID");
    unmount();

    vi.mocked(api.readCapture).mockResolvedValueOnce({ ...capture, body_text: null, truncated: true });
    render(<CaptureDetail listener={listener()} captureId="c1" />);
    expect(await screen.findByLabelText("Body")).toHaveTextContent("exceeded the limit");
  });
});

describe("WebhookTab", () => {
  it("shows url, tunnel hint, live captures and opens detail", async () => {
    vi.mocked(api.listCaptures).mockResolvedValue([capture.summary]);
    useWebhookStore.setState({
      listeners: [listener({ running: true, url: "http://127.0.0.1:9123/hooks" })],
    });
    render(<WebhookTab webhookId="a" />);

    expect(screen.getByText("http://127.0.0.1:9123/hooks")).toBeVisible();
    expect(screen.getByText(/ngrok http 9123/)).toBeVisible();
    const list = await screen.findByRole("list", { name: "Captured requests" });
    await userEvent.click(within(list).getByRole("button", { name: /POST/ }));
    expect(await screen.findByRole("region", { name: "Capture detail" })).toBeVisible();

    await userEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(screen.getByRole("form", { name: "Listener settings" })).toBeVisible();
  });

  it("explains when the listener is gone", () => {
    render(<WebhookTab webhookId="zzz" />);
    expect(screen.getByText("This listener no longer exists.")).toBeVisible();
  });
});
