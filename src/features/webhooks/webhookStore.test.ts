import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CaptureSummary, WebhookListener } from "../../ipc/bindings";
import { useTabsStore } from "../../shell/tabsStore";
import { appError } from "../../test/fixtures";
import { useToastStore } from "../../ui/toastStore";
import { scratchLocation } from "../locations";
import { useScratchStore } from "../scratch/scratchStore";
import * as api from "./api";
import { DEFAULT_CONFIG } from "./schema";
import { useWebhookStore } from "./webhookStore";

vi.mock("./api");
vi.mock("../scratch/api", () => ({ listScratch: vi.fn().mockResolvedValue([]) }));

const listener = (id: string, patch: Partial<WebhookListener> = {}): WebhookListener => ({
  config: { ...DEFAULT_CONFIG, id, name: `L${id}` },
  running: false,
  url: null,
  captured: 0,
  unmatched: 0,
  ...patch,
});
const summary = (id: string): CaptureSummary => ({
  id,
  at_ms: 1,
  method: "POST",
  path: "/hooks",
  size: 2,
  content_type: "",
});
const state = () => useWebhookStore.getState();

describe("webhookStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useWebhookStore.setState({ listeners: [], captures: {} });
    useTabsStore.setState(useTabsStore.getInitialState(), true);
    useToastStore.setState({ toasts: [] });
    useScratchStore.setState({ items: [] });
  });

  it("load stores listeners; failure reports a toast", async () => {
    vi.mocked(api.listWebhooks).mockResolvedValueOnce([listener("a")]);
    await state().load();
    expect(state().listeners).toHaveLength(1);

    vi.mocked(api.listWebhooks).mockRejectedValueOnce(appError("x"));
    await state().load();
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });

  it("create saves the defaults and returns the new listener", async () => {
    vi.mocked(api.saveWebhook).mockResolvedValue(listener("n"));
    const created = await state().create();
    expect(api.saveWebhook).toHaveBeenCalledWith(DEFAULT_CONFIG);
    expect(created?.config.id).toBe("n");
    expect(state().listeners).toHaveLength(1);
  });

  it("create refuses past 20 listeners", async () => {
    useWebhookStore.setState({ listeners: Array.from({ length: 20 }, (_, i) => listener(`l${i}`)) });
    expect(await state().create()).toBeNull();
    expect(api.saveWebhook).not.toHaveBeenCalled();
  });

  it("a failed save re-lists listeners so running state reflects reality", async () => {
    useWebhookStore.setState({ listeners: [listener("a", { running: true, url: "http://x" })] });
    vi.mocked(api.saveWebhook).mockRejectedValue(appError("nope"));
    vi.mocked(api.listWebhooks).mockResolvedValue([listener("a", { running: false })]);
    expect(await state().save(listener("a").config)).toBe(false);
    expect(state().listeners[0]?.running).toBe(false);
  });

  it("save replaces the listener and retitles its tab", async () => {
    useWebhookStore.setState({ listeners: [listener("a")] });
    useTabsStore.getState().openWebhook("a", "La");
    const next = listener("a", { running: true });
    vi.mocked(api.saveWebhook).mockResolvedValue({ ...next, config: { ...next.config, name: "Renamed" } });

    expect(await state().save(next.config)).toBe(true);
    expect(state().listeners[0].config.name).toBe("Renamed");
    expect(useTabsStore.getState().tabs[1].title).toBe("Renamed");
  });

  it("remove drops the listener, captures and tab; a failure keeps them", async () => {
    useWebhookStore.setState({ listeners: [listener("a")], captures: { a: [summary("c1")] } });
    useTabsStore.getState().openWebhook("a", "La");
    vi.mocked(api.deleteWebhook).mockRejectedValueOnce(appError("x"));
    expect(await state().remove("a")).toBe(false);
    expect(state().listeners).toHaveLength(1);

    vi.mocked(api.deleteWebhook).mockResolvedValueOnce(null);
    expect(await state().remove("a")).toBe(true);
    expect(state().listeners).toHaveLength(0);
    expect(state().captures).toEqual({});
    expect(useTabsStore.getState().tabs).toHaveLength(1);
  });

  it("start stores the running listener; stop clears running and url", async () => {
    useWebhookStore.setState({ listeners: [listener("a")] });
    vi.mocked(api.startWebhook).mockResolvedValue(listener("a", { running: true, url: "http://127.0.0.1:9/hooks" }));
    await state().start("a");
    expect(state().listeners[0].running).toBe(true);

    vi.mocked(api.stopWebhook).mockResolvedValue(null);
    await state().stop("a");
    expect(state().listeners[0]).toMatchObject({ running: false, url: null });
  });

  it("loadCaptures shows the newest first", async () => {
    vi.mocked(api.listCaptures).mockResolvedValue([summary("c1"), summary("c2")]);
    await state().loadCaptures("a");
    expect(state().captures.a.map((c) => c.id)).toEqual(["c2", "c1"]);
  });

  it("handleCaptured prepends, counts, ignores duplicates and trims to max_captures", () => {
    const small = listener("a");
    useWebhookStore.setState({
      listeners: [{ ...small, config: { ...small.config, max_captures: 2 } }],
    });
    state().handleCaptured({ listener_id: "a", capture: summary("c1") });
    state().handleCaptured({ listener_id: "a", capture: summary("c1") });
    state().handleCaptured({ listener_id: "a", capture: summary("c2") });
    state().handleCaptured({ listener_id: "a", capture: summary("c3") });

    expect(state().captures.a.map((c) => c.id)).toEqual(["c3", "c2"]);
    expect(state().listeners[0].captured).toBe(3);
  });

  it("handleStopped marks the listener stopped and reports an error", () => {
    useWebhookStore.setState({ listeners: [listener("a", { running: true, url: "http://x" })] });
    state().handleStopped({ listener_id: "a", error: appError("Port gone") });
    expect(state().listeners[0].running).toBe(false);
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });

  it("clearCaptures empties the list and counters", async () => {
    useWebhookStore.setState({ listeners: [listener("a", { captured: 4 })], captures: { a: [summary("c1")] } });
    vi.mocked(api.clearCaptures).mockResolvedValue(null);
    await state().clearCaptures("a");
    expect(state().captures.a).toEqual([]);
    expect(state().listeners[0].captured).toBe(0);
  });

  it("replay opens the returned scratch item and toasts the warnings", async () => {
    vi.mocked(api.replayCapture).mockResolvedValue({
      item: { id: "s9", name: "POST /hooks 12:04:05", method: "POST", kind: "http" },
      warnings: ["Binary body dropped."],
    });
    expect(await state().replay("a", "c1")).toBe(true);

    const tab = useTabsStore.getState().tabs[1];
    expect(tab.location).toEqual(scratchLocation("s9"));
    expect(tab.title).toBe("POST /hooks 12:04:05");
    expect(useToastStore.getState().toasts[0].message).toContain("Binary body dropped.");
  });

  it("replay failure reports and opens nothing", async () => {
    vi.mocked(api.replayCapture).mockRejectedValue(appError("x"));
    expect(await state().replay("a", "c1")).toBe(false);
    expect(useTabsStore.getState().tabs).toHaveLength(1);
  });

  it("subscribe wires both events to the handlers and unsubscribes", async () => {
    const off = vi.fn();
    vi.mocked(api.subscribeCaptured).mockImplementation(async (cb) => {
      cb({ listener_id: "a", capture: summary("c1") });
      return off;
    });
    vi.mocked(api.subscribeStopped).mockResolvedValue(off);
    const unsubscribe = await state().subscribe();
    expect(state().captures.a).toHaveLength(1);
    unsubscribe();
    expect(off).toHaveBeenCalledTimes(2);
  });
});
