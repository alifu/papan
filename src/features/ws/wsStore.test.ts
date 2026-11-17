import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WsEvent, WsMessage, WsOpened } from "../../ipc/bindings";
import { scratchLocation } from "../locations";
import { emptyContent } from "../request/content";
import * as api from "./api";
import { LOG_CAP } from "./wsModel";
import { CLOSE_TIMEOUT_MS, useWsStore } from "./wsStore";

vi.mock("./api");
vi.mock("../../ui/toastStore", () => ({ reportError: vi.fn() }));

const loc = scratchLocation("s1");
const CID = "11111111-1111-4111-8111-111111111111";
const bound = () => useWsStore.getState().restore("t", loc);
const opened = (over: Partial<WsOpened> = {}): WsOpened => ({
  connection_id: CID,
  url: "wss://x/ws",
  protocol: "chat.v2",
  status: 101,
  headers: [],
  ...over,
});
const msg = (seq: number, over: Partial<WsMessage> = {}): WsMessage => ({
  seq,
  at_ms: 1,
  direction: "received",
  frame: "text",
  text: "hi",
  size: 2,
  truncated: false,
  ...over,
});
const state = () => useWsStore.getState().byTab.t;
let emit: (e: WsEvent) => void = () => {};

describe("wsStore", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.spyOn(crypto, "randomUUID").mockReturnValue(CID);
    vi.useRealTimers();
    useWsStore.getState().reset();
    vi.mocked(api.listenWsEvents).mockImplementation((cb) => {
      emit = cb;
      return Promise.resolve(() => {});
    });
  });

  it("connects: connecting, then open with the negotiated protocol", async () => {
    let finish: (o: WsOpened) => void = () => {};
    vi.mocked(api.wsConnect).mockReturnValue(new Promise((r) => (finish = r)));
    const pending = useWsStore.getState().connect("t", loc, emptyContent());
    expect(state()?.status).toBe("connecting");
    await vi.waitFor(() => expect(api.wsConnect).toHaveBeenCalledWith(CID, loc, emptyContent()));
    finish(opened());
    await pending;
    expect(state()).toMatchObject({ status: "open", opened: { protocol: "chat.v2" }, error: null });
  });

  it("keeps an error and goes to closed when the handshake fails", async () => {
    vi.mocked(api.wsConnect).mockRejectedValue({ code: "network", message: "refused", detail: null });
    await useWsStore.getState().connect("t", loc, emptyContent());
    expect(state()).toMatchObject({ status: "closed", error: { message: "refused" } });
  });

  it("starts the event listener once", async () => {
    vi.mocked(api.wsConnect).mockResolvedValue(opened());
    await useWsStore.getState().connect("t", loc, emptyContent());
    await useWsStore.getState().connect("t", loc, emptyContent());
    expect(api.listenWsEvents).toHaveBeenCalledTimes(1);
  });

  it("applies message events for its own connection only, deduplicating by seq", async () => {
    vi.mocked(api.wsConnect).mockResolvedValue(opened());
    await useWsStore.getState().connect("t", loc, emptyContent());
    emit({ connection_id: CID, kind: { type: "messages", items: [msg(1), msg(2)] } });
    emit({ connection_id: CID, kind: { type: "messages", items: [msg(2), msg(3)] } });
    emit({ connection_id: "other", kind: { type: "messages", items: [msg(9)] } });
    expect(state()?.log.map((m) => m.seq)).toEqual([1, 2, 3]);
    expect(useWsStore.getState().byTab.other).toBeUndefined();
  });

  it("an opened event moves a connecting tab to open", async () => {
    vi.mocked(api.wsLog).mockResolvedValue([]);
    await bound();
    useWsStore.getState().apply({ connection_id: CID, kind: { type: "opened", ...opened() } });
    expect(state()).toMatchObject({ status: "open", opened: { status: 101 } });
  });

  it("a closed event records who closed and why", async () => {
    vi.mocked(api.wsConnect).mockResolvedValue(opened());
    await useWsStore.getState().connect("t", loc, emptyContent());
    emit({ connection_id: CID, kind: { type: "closed", code: 1001, reason: "bye", by: "server", error: null } });
    expect(state()).toMatchObject({ status: "closed", closed: { code: 1001, reason: "bye", by: "server" } });
  });

  it("a closed event with an error keeps the error", async () => {
    vi.mocked(api.wsLog).mockResolvedValue([]);
    await bound();
    const error = { code: "network" as const, message: "reset", detail: null };
    useWsStore.getState().apply({ connection_id: CID, kind: { type: "closed", code: null, reason: "", by: "error", error } });
    expect(state()?.error).toEqual(error);
  });

  it("caps the log", async () => {
    vi.mocked(api.wsLog).mockResolvedValue([]);
    await bound();
    const items = Array.from({ length: LOG_CAP + 3 }, (_, i) => msg(i + 1));
    useWsStore.getState().apply({ connection_id: CID, kind: { type: "messages", items } });
    expect(state()?.log).toHaveLength(LOG_CAP);
  });

  it("disconnect stays closing until the Closed event", async () => {
    vi.mocked(api.wsConnect).mockResolvedValue(opened());
    vi.mocked(api.wsDisconnect).mockResolvedValue(null);
    await useWsStore.getState().connect("t", loc, emptyContent());
    await useWsStore.getState().disconnect("t");
    expect(api.wsDisconnect).toHaveBeenCalledWith(CID, 1000, "");
    expect(state()?.status).toBe("closing");
    emit({ connection_id: CID, kind: { type: "closed", code: 1000, reason: "", by: "client", error: null } });
    expect(state()?.status).toBe("closed");
  });

  it("disconnect falls back to closed after the timeout", async () => {
    vi.mocked(api.wsConnect).mockResolvedValue(opened());
    vi.mocked(api.wsDisconnect).mockResolvedValue(null);
    await useWsStore.getState().connect("t", loc, emptyContent());
    vi.useFakeTimers();
    await useWsStore.getState().disconnect("t");
    vi.advanceTimersByTime(CLOSE_TIMEOUT_MS);
    expect(state()?.status).toBe("closed");
  });

  it("a cancelled connect goes to closed without an error", async () => {
    vi.mocked(api.wsConnect).mockRejectedValue({ code: "cancelled", message: "cancelled", detail: null });
    await useWsStore.getState().connect("t", loc, emptyContent());
    expect(state()).toMatchObject({ status: "closed", error: null });
  });

  it("reuses one connection id per location across tabs", async () => {
    vi.mocked(api.wsLog).mockResolvedValue([msg(1)]);
    await useWsStore.getState().restore("t", loc);
    useWsStore.getState().release("t");
    vi.mocked(crypto.randomUUID).mockReturnValue("22222222-2222-4222-8222-222222222222");
    await useWsStore.getState().restore("t2", loc);
    expect(api.wsLog).toHaveBeenLastCalledWith(CID);
    expect(useWsStore.getState().byTab.t2?.log).toHaveLength(1);
  });

  it("release disconnects a live connection, drops state and ignores later events", async () => {
    vi.mocked(api.wsConnect).mockResolvedValue(opened());
    vi.mocked(api.wsDisconnect).mockResolvedValue(null);
    await useWsStore.getState().connect("t", loc, emptyContent());
    await useWsStore.getState().release("t");
    expect(api.wsDisconnect).toHaveBeenCalledWith(CID, 1000, "");
    expect(state()).toBeUndefined();
    emit({ connection_id: CID, kind: { type: "messages", items: [msg(1)] } });
    expect(state()).toBeUndefined();
  });

  it("release of a closed or unknown tab does not call disconnect", async () => {
    await useWsStore.getState().release("nope");
    vi.mocked(api.wsConnect).mockRejectedValue({ code: "network", message: "x", detail: null });
    await useWsStore.getState().connect("t", loc, emptyContent());
    await useWsStore.getState().release("t");
    expect(api.wsDisconnect).not.toHaveBeenCalled();
  });

  it("a connect finishing after release does not resurrect the tab", async () => {
    let finish: (o: WsOpened) => void = () => {};
    vi.mocked(api.wsConnect).mockReturnValue(new Promise((r) => (finish = r)));
    vi.mocked(api.wsDisconnect).mockResolvedValue(null);
    const pending = useWsStore.getState().connect("t", loc, emptyContent());
    await vi.waitFor(() => expect(api.wsConnect).toHaveBeenCalled());
    await useWsStore.getState().release("t");
    finish(opened());
    await pending;
    expect(api.wsDisconnect).toHaveBeenCalledTimes(1);
    expect(state()).toBeUndefined();
  });

  it("send adds the returned entry to the log", async () => {
    vi.mocked(api.wsSend).mockResolvedValue(msg(1, { direction: "sent", text: "{}" }));
    vi.mocked(api.wsLog).mockResolvedValue([]);
    await bound();
    await useWsStore.getState().send("t", loc, { frame: "json", text: "{}" });
    expect(api.wsSend).toHaveBeenCalledWith(CID, loc, { frame: "json", text: "{}" });
    expect(state()?.log[0]?.direction).toBe("sent");
  });

  it("send failure leaves the log alone and reports", async () => {
    vi.mocked(api.wsSend).mockRejectedValue({ code: "network", message: "closed", detail: null });
    await useWsStore.getState().send("t", loc, { frame: "text", text: "x" });
    expect(state()?.log ?? []).toHaveLength(0);
  });

  it("clear empties the log through the backend", async () => {
    vi.mocked(api.wsClearLog).mockResolvedValue(null);
    vi.mocked(api.wsLog).mockResolvedValue([]);
    await bound();
    useWsStore.getState().apply({ connection_id: CID, kind: { type: "messages", items: [msg(1)] } });
    await useWsStore.getState().clear("t");
    expect(api.wsClearLog).toHaveBeenCalledWith(CID);
    expect(state()?.log).toEqual([]);
  });

  it("restore re-shows the backend log of a re-opened tab once", async () => {
    vi.mocked(api.wsLog).mockResolvedValue([msg(1), msg(2)]);
    await useWsStore.getState().restore("t", loc);
    await useWsStore.getState().restore("t", loc);
    expect(api.wsLog).toHaveBeenCalledTimes(1);
    expect(state()?.log).toHaveLength(2);
  });

  it("composer edits are immutable patches", () => {
    useWsStore.getState().setComposer("t", { text: "a" });
    useWsStore.getState().setComposer("t", { format: "json" });
    expect(state()?.composer).toEqual({ format: "json", text: "a" });
  });

  it("drop forgets the tab", () => {
    useWsStore.getState().setComposer("t", { text: "a" });
    useWsStore.getState().drop("t");
    expect(state()).toBeUndefined();
  });

  describe("subscriptions", () => {
    it("subscribe goes connecting, then open through graphqlSubscribe", async () => {
      vi.mocked(api.graphqlSubscribe).mockResolvedValue(opened({ protocol: "graphql-transport-ws" }));
      await useWsStore.getState().subscribe("t", loc, emptyContent());
      expect(api.graphqlSubscribe).toHaveBeenCalledWith(CID, loc, emptyContent());
      expect(api.wsConnect).not.toHaveBeenCalled();
      expect(state()).toMatchObject({ status: "open", opened: { protocol: "graphql-transport-ws" } });
    });

    it("a failed start keeps the error", async () => {
      vi.mocked(api.graphqlSubscribe).mockRejectedValue({ code: "network", message: "4401 unauthorized", detail: null });
      await useWsStore.getState().subscribe("t", loc, emptyContent());
      expect(state()).toMatchObject({ status: "closed", error: { message: "4401 unauthorized" } });
    });

    it("a user-cancelled start is not an error", async () => {
      vi.mocked(api.graphqlSubscribe).mockRejectedValue({ code: "cancelled", message: "Cancelled", detail: null });
      await useWsStore.getState().subscribe("t", loc, emptyContent());
      expect(state()).toMatchObject({ status: "closed", error: null });
    });

    it("unsubscribe stays closing until the Closed event", async () => {
      vi.mocked(api.graphqlSubscribe).mockResolvedValue(opened());
      vi.mocked(api.graphqlUnsubscribe).mockResolvedValue(null);
      await useWsStore.getState().subscribe("t", loc, emptyContent());
      await useWsStore.getState().unsubscribe("t");
      expect(api.graphqlUnsubscribe).toHaveBeenCalledWith(CID);
      expect(state()?.status).toBe("closing");
      emit({ connection_id: CID, kind: { type: "closed", code: 1000, reason: "", by: "client", error: null } });
      expect(state()?.status).toBe("closed");
    });

    it("unsubscribe failure restores the open state and reports", async () => {
      vi.mocked(api.graphqlSubscribe).mockResolvedValue(opened());
      vi.mocked(api.graphqlUnsubscribe).mockRejectedValue({ code: "internal", message: "x", detail: null });
      await useWsStore.getState().subscribe("t", loc, emptyContent());
      await useWsStore.getState().unsubscribe("t");
      expect(state()?.status).toBe("open");
    });

    it("ignores events for unknown ids and releases on tab close", async () => {
      vi.mocked(api.graphqlSubscribe).mockResolvedValue(opened());
      vi.mocked(api.wsDisconnect).mockResolvedValue(null);
      await useWsStore.getState().subscribe("t", loc, emptyContent());
      emit({ connection_id: "stale", kind: { type: "messages", items: [msg(1)] } });
      expect(state()?.log).toEqual([]);
      await useWsStore.getState().release("t");
      expect(api.wsDisconnect).toHaveBeenCalledWith(CID, 1000, "");
      expect(state()).toBeUndefined();
    });
  });
});
