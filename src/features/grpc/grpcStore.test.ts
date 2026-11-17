import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GrpcEvent, GrpcEventKind, GrpcSchema, GrpcStarted, GrpcStatus, WsMessage } from "../../ipc/bindings";
import { scratchLocation } from "../locations";
import { emptyContent } from "../request/content";
import { reportError } from "../../ui/toastStore";
import * as api from "./api";
import { CANCEL_TIMEOUT_MS, useGrpcStore } from "./grpcStore";

vi.mock("./api");
vi.mock("../../ui/toastStore", () => ({ reportError: vi.fn() }));

const loc = scratchLocation("s1");
const CID = "11111111-1111-4111-8111-111111111111";
const started = (attempt = 1): GrpcStarted => ({ call_id: CID, attempt, path: "/a.G/Say", kind: "unary", url: "https://x" });
const msg = (seq: number, over: Partial<WsMessage> = {}): WsMessage => ({
  seq,
  at_ms: 1,
  direction: "received",
  frame: "text",
  text: "{}",
  size: 2,
  truncated: false,
  ...over,
});
const status = (code: number, name: string): GrpcStatus => ({ code, name, message: "m", details_base64: null });
const finished = (over: Partial<Extract<GrpcEventKind, { type: "finished" }>> = {}): GrpcEventKind => ({
  type: "finished",
  status: status(0, "OK"),
  trailers: [{ name: "t", value: "1", enabled: true }],
  by: "server",
  elapsed_ms: 5,
  error: null,
  ...over,
});
const event = (kind: GrpcEventKind, attempt = 1, call_id = CID): GrpcEvent => ({ call_id, attempt, kind });
const tab = () => useGrpcStore.getState().byTab.t;
let emit: (e: GrpcEvent) => void = () => {};
const startCall = async () => {
  vi.mocked(api.grpcStart).mockResolvedValue(started());
  await useGrpcStore.getState().start("t", loc, emptyContent());
};

describe("grpcStore", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.spyOn(crypto, "randomUUID").mockReturnValue(CID);
    vi.useRealTimers();
    useGrpcStore.getState().reset();
    vi.mocked(api.listenGrpcEvents).mockImplementation((cb) => {
      emit = cb;
      return Promise.resolve(() => {});
    });
  });

  it("starts: connecting, then open with the started info", async () => {
    let finish: (s: GrpcStarted) => void = () => {};
    vi.mocked(api.grpcStart).mockReturnValue(new Promise((r) => (finish = r)));
    const pending = useGrpcStore.getState().start("t", loc, emptyContent());
    expect(tab()?.status).toBe("connecting");
    await vi.waitFor(() => expect(api.grpcStart).toHaveBeenCalledWith(CID, loc, emptyContent()));
    finish(started());
    await pending;
    expect(tab()).toMatchObject({ status: "open", attempt: 1, started: { path: "/a.G/Say" }, error: null });
  });

  it("keeps the same call id per location and clears the previous result on restart", async () => {
    await startCall();
    emit(event({ type: "messages", items: [msg(1)] }));
    emit(event(finished()));
    vi.mocked(crypto.randomUUID).mockReturnValue("22222222-2222-4222-8222-222222222222");
    vi.mocked(api.grpcStart).mockResolvedValue(started(2));
    await useGrpcStore.getState().start("t", loc, emptyContent());
    expect(api.grpcStart).toHaveBeenLastCalledWith(CID, loc, emptyContent());
    expect(tab()).toMatchObject({ status: "open", attempt: 2, grpcStatus: null, trailers: [], headers: null });
  });

  it("shows a start failure as finished with the error", async () => {
    vi.mocked(api.grpcStart).mockRejectedValue({ code: "network", message: "refused", detail: null });
    await useGrpcStore.getState().start("t", loc, emptyContent());
    expect(tab()).toMatchObject({ status: "finished", error: { message: "refused" } });
  });

  it("does not show a cancelled start as an error", async () => {
    vi.mocked(api.grpcStart).mockRejectedValue({ code: "cancelled", message: "Cancelled.", detail: null });
    await useGrpcStore.getState().start("t", loc, emptyContent());
    expect(tab()).toMatchObject({ status: "idle", error: null });
  });

  it("shows the messages of a second run although it restarts at seq 1", async () => {
    await startCall();
    emit(event({ type: "messages", items: [msg(1, { text: "first" })] }));
    emit(event(finished()));
    vi.mocked(api.grpcStart).mockResolvedValue(started(2));
    await useGrpcStore.getState().start("t", loc, emptyContent());
    expect(tab()?.log).toEqual([]);
    emit(event({ type: "messages", items: [msg(1, { text: "second" })] }, 2));
    expect(tab()?.log.map((m) => m.text)).toEqual(["second"]);
  });

  it("falls back to an internal error for an unknown failure", async () => {
    vi.mocked(api.grpcStart).mockRejectedValue(new Error("boom"));
    await useGrpcStore.getState().start("t", loc, emptyContent());
    expect(tab()?.error?.code).toBe("internal");
  });

  it("applies headers, messages and finished events in order", async () => {
    await startCall();
    emit(event({ type: "headers", metadata: [{ name: "h", value: "v", enabled: true }] }));
    emit(event({ type: "messages", items: [msg(1), msg(2)] }));
    emit(event({ type: "messages", items: [msg(2), msg(3)] }));
    expect(tab()).toMatchObject({ headers: [{ name: "h", value: "v", enabled: true }] });
    expect(tab()?.log.map((m) => m.seq)).toEqual([1, 2, 3]);
    emit(event(finished()));
    expect(tab()).toMatchObject({ status: "finished", grpcStatus: { name: "OK" }, trailers: [{ name: "t", value: "1", enabled: true }], elapsedMs: 5, by: "server" });
  });

  it("keeps the log at the cap", async () => {
    await startCall();
    emit(event({ type: "messages", items: Array.from({ length: 1005 }, (_, i) => msg(i + 1)) }));
    expect(tab()?.log).toHaveLength(1000);
    expect(tab()?.log[0]?.seq).toBe(6);
  });

  it("ignores events for unknown ids and for stale attempts", async () => {
    await startCall();
    emit(event({ type: "messages", items: [msg(1)] }, 1, "other"));
    expect(tab()?.log).toEqual([]);
    vi.mocked(api.grpcStart).mockResolvedValue(started(2));
    await useGrpcStore.getState().start("t", loc, emptyContent());
    emit(event(finished({ status: status(14, "UNAVAILABLE") }), 1));
    expect(tab()).toMatchObject({ status: "open", grpcStatus: null });
  });

  it("ignores the old attempt's Finished while a restart is pending", async () => {
    await startCall();
    vi.mocked(api.grpcStart).mockReturnValue(new Promise(() => {}));
    void useGrpcStore.getState().start("t", loc, emptyContent());
    emit(event(finished(), 1));
    expect(tab()?.status).toBe("connecting");
  });

  it("adopts the attempt of an event that arrives before start resolves", async () => {
    vi.mocked(api.grpcStart).mockReturnValue(new Promise(() => {}));
    void useGrpcStore.getState().start("t", loc, emptyContent());
    await vi.waitFor(() => expect(api.grpcStart).toHaveBeenCalled());
    emit(event({ type: "headers", metadata: [] }, 1));
    expect(tab()).toMatchObject({ attempt: 1, headers: [] });
  });

  it("does not alert when a cancelled call finishes", async () => {
    await startCall();
    emit(event(finished({ status: status(1, "CANCELLED"), by: "client" })));
    expect(tab()).toMatchObject({ status: "finished", error: null });
  });

  it("keeps the error of a failed call", async () => {
    await startCall();
    emit(event(finished({ status: status(14, "UNAVAILABLE"), by: "error", error: { code: "network", message: "down", detail: null } })));
    expect(tab()?.error?.message).toBe("down");
  });

  it("cancel goes to closing and waits for the finished event", async () => {
    await startCall();
    vi.mocked(api.grpcCancel).mockResolvedValue(null);
    await useGrpcStore.getState().cancel("t");
    expect(api.grpcCancel).toHaveBeenCalledWith(CID);
    expect(tab()?.status).toBe("closing");
    emit(event(finished({ status: status(1, "CANCELLED"), by: "client" })));
    expect(tab()?.status).toBe("finished");
  });

  it("falls back to finished when the finished event never comes", async () => {
    vi.useFakeTimers();
    await startCall();
    vi.mocked(api.grpcCancel).mockResolvedValue(null);
    await useGrpcStore.getState().cancel("t");
    vi.advanceTimersByTime(CANCEL_TIMEOUT_MS);
    expect(tab()?.status).toBe("finished");
  });

  it("restores the previous status and reports when cancel fails", async () => {
    await startCall();
    vi.mocked(api.grpcCancel).mockRejectedValue(new Error("x"));
    await useGrpcStore.getState().cancel("t");
    expect(tab()?.status).toBe("open");
    expect(reportError).toHaveBeenCalled();
  });

  it("sends a message and logs the returned entry", async () => {
    await startCall();
    vi.mocked(api.grpcSend).mockResolvedValue(msg(7, { direction: "sent" }));
    await useGrpcStore.getState().send("t", loc, '{"a":1}');
    expect(api.grpcSend).toHaveBeenCalledWith(CID, loc, '{"a":1}');
    expect(tab()?.log.map((m) => m.seq)).toEqual([7]);
  });

  it("reports a failed send without changing the call", async () => {
    await startCall();
    vi.mocked(api.grpcSend).mockRejectedValue(new Error("x"));
    await useGrpcStore.getState().send("t", loc, "{}");
    expect(reportError).toHaveBeenCalled();
    expect(tab()?.status).toBe("open");
  });

  it("end half-closes", async () => {
    await startCall();
    vi.mocked(api.grpcEnd).mockResolvedValue(null);
    await useGrpcStore.getState().end("t");
    expect(tab()?.status).toBe("half_closed");
  });

  it("end does not override a call that finished meanwhile", async () => {
    await startCall();
    vi.mocked(api.grpcEnd).mockImplementation(() => {
      emit(event(finished()));
      return Promise.resolve(null);
    });
    await useGrpcStore.getState().end("t");
    expect(tab()?.status).toBe("finished");
  });

  it("clears the log", async () => {
    await startCall();
    emit(event({ type: "messages", items: [msg(1)] }));
    vi.mocked(api.grpcClearLog).mockResolvedValue(null);
    await useGrpcStore.getState().clear("t");
    expect(api.grpcClearLog).toHaveBeenCalledWith(CID);
    expect(tab()?.log).toEqual([]);
  });

  it("restores the backend log once for a re-opened tab", async () => {
    vi.mocked(api.grpcLog).mockResolvedValue([msg(1), msg(2)]);
    await useGrpcStore.getState().restore("t", loc);
    await useGrpcStore.getState().restore("t", loc);
    expect(api.grpcLog).toHaveBeenCalledTimes(1);
    expect(api.grpcLog).toHaveBeenCalledWith(CID);
    expect(tab()?.log).toHaveLength(2);
  });

  it("ignores a failed restore", async () => {
    vi.mocked(api.grpcLog).mockRejectedValue(new Error("none"));
    await useGrpcStore.getState().restore("t", loc);
    expect(tab()?.log).toEqual([]);
  });

  it("loads a schema and ignores an older response that arrives late", async () => {
    const schema = (n: string): GrpcSchema => ({ source: "protos", services: [{ full_name: n, methods: [] }], files: [], warnings: [], fetched_at_ms: null });
    let first: (s: GrpcSchema) => void = () => {};
    vi.mocked(api.grpcSchema).mockReturnValueOnce(new Promise((r) => (first = r))).mockResolvedValueOnce(schema("new"));
    const slow = useGrpcStore.getState().loadSchema("t", loc, emptyContent());
    await useGrpcStore.getState().loadSchema("t", loc, emptyContent());
    first(schema("old"));
    await slow;
    expect(tab()?.schema?.services[0]?.full_name).toBe("new");
    expect(tab()?.schemaLoading).toBe(false);
  });

  it("auto-load never allows the network and flags a missing cache as needing a refresh", async () => {
    vi.mocked(api.grpcSchema).mockResolvedValue(null);
    await useGrpcStore.getState().loadSchema("t", loc, emptyContent());
    expect(api.grpcSchema).toHaveBeenCalledWith(loc, emptyContent(), false);
    expect(tab()).toMatchObject({ schema: null, schemaNeedsRefresh: true, schemaError: null });
  });

  it("keeps the schema error and clears the schema on failure", async () => {
    vi.mocked(api.grpcSchema).mockRejectedValue({ code: "invalid_input", message: "a.proto:3:1 bad", detail: null });
    await useGrpcStore.getState().loadSchema("t", loc, emptyContent());
    expect(tab()).toMatchObject({ schema: null, schemaError: { message: "a.proto:3:1 bad" }, schemaLoading: false });
  });

  it("reflects with refresh", async () => {
    vi.mocked(api.grpcReflect).mockResolvedValue({ source: "reflection", services: [], files: [], warnings: [], fetched_at_ms: 9 });
    await useGrpcStore.getState().reflect("t", loc, emptyContent());
    expect(api.grpcReflect).toHaveBeenCalledWith(loc, emptyContent(), true);
    expect(tab()?.schema?.fetched_at_ms).toBe(9);
  });

  it("release cancels a live call and forgets the tab; a finished one is only forgotten", async () => {
    await startCall();
    vi.mocked(api.grpcCancel).mockResolvedValue(null);
    await useGrpcStore.getState().release("t");
    expect(api.grpcCancel).toHaveBeenCalledWith(CID);
    expect(tab()).toBeUndefined();
    expect(useGrpcStore.getState().callByTab.t).toBeUndefined();

    vi.mocked(api.grpcCancel).mockClear();
    await startCall();
    emit(event(finished()));
    await useGrpcStore.getState().release("t");
    expect(api.grpcCancel).not.toHaveBeenCalled();
  });

  it("release swallows a cancel failure and events after it are ignored", async () => {
    await startCall();
    vi.mocked(api.grpcCancel).mockRejectedValue(new Error("gone"));
    await useGrpcStore.getState().release("t");
    emit(event({ type: "messages", items: [msg(1)] }));
    expect(tab()).toBeUndefined();
  });

  it("reset unlistens the event stream", async () => {
    const unlisten = vi.fn();
    vi.mocked(api.listenGrpcEvents).mockResolvedValue(unlisten);
    await startCall();
    useGrpcStore.getState().reset();
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalled());
  });

  it("routes events to every tab showing the same call id", async () => {
    await startCall();
    useGrpcStore.setState((s) => ({ callByTab: { ...s.callByTab, u: CID }, byTab: { ...s.byTab, u: s.byTab.t } }));
    emit(event({ type: "messages", items: [msg(1)] }));
    expect(useGrpcStore.getState().byTab.u?.log).toHaveLength(1);
    expect(tab()?.log).toHaveLength(1);
  });

  it("a start that resolves after release does not recreate the tab", async () => {
    let finish: (s: GrpcStarted) => void = () => {};
    vi.mocked(api.grpcStart).mockReturnValue(new Promise((r) => (finish = r)));
    vi.mocked(api.grpcCancel).mockResolvedValue(null);
    const pending = useGrpcStore.getState().start("t", loc, emptyContent());
    await vi.waitFor(() => expect(api.grpcStart).toHaveBeenCalled());
    await useGrpcStore.getState().release("t");
    finish(started());
    await pending;
    expect(tab()).toBeUndefined();
  });
});
