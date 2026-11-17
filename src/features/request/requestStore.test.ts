import { beforeEach, describe, expect, it, vi } from "vitest";
import { useToastStore } from "../../ui/toastStore";
import * as collections from "../collections/api";
import { scratchLocation } from "../locations";
import * as api from "./api";
import { emptyContent } from "./content";
import { isDirty, useRequestStore } from "./requestStore";

vi.mock("../collections/api");
vi.mock("./api");

const loc = scratchLocation("s1");
const state = () => useRequestStore.getState();
const doc = { name: "R", content: { ...emptyContent(), url: "http://x?a=1" } };
const meta = { request_id: "r" } as never;

describe("requestStore", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useRequestStore.setState({ byTab: {} });
    useToastStore.setState({ toasts: [] });
    vi.mocked(collections.readRequest).mockResolvedValue(doc);
  });

  it("is clean after load and dirty after an edit; reverting clears it", async () => {
    await state().load("t", loc);
    expect(isDirty("t")).toBe(false);
    expect(state().byTab.t.paramRows).toHaveLength(1);

    state().editUrl("t", "http://x?a=2");
    expect(isDirty("t")).toBe(true);
    state().editUrl("t", "http://x?a=1");
    expect(isDirty("t")).toBe(false);
    expect(doc.content.url).toBe("http://x?a=1");
  });

  it("table edits rewrite the URL and move disabled rows to disabled_params", async () => {
    await state().load("t", loc);
    state().editParams("t", [{ name: "a", value: "1", enabled: false }]);

    expect(state().byTab.t.draft).toMatchObject({ url: "http://x", disabled_params: [{ name: "a", value: "1" }] });
  });

  it("saves the draft and clears dirty; a failure keeps it and toasts", async () => {
    vi.mocked(collections.writeRequest).mockResolvedValueOnce(null);
    await state().load("t", loc);
    state().editUrl("t", "http://y");
    await state().save("t", loc);
    expect(collections.writeRequest).toHaveBeenCalledWith(loc, expect.objectContaining({ url: "http://y" }));
    expect(isDirty("t")).toBe(false);

    vi.mocked(collections.writeRequest).mockRejectedValueOnce({ code: "io", message: "disk" });
    state().editUrl("t", "http://z");
    await state().save("t", loc);
    expect(isDirty("t")).toBe(true);
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });

  it("sends the draft with a request id, then stores the meta", async () => {
    vi.mocked(api.sendRequestOutcome).mockResolvedValue({ meta, scripts: [], error: null });
    await state().load("t", loc);
    state().editUrl("t", "http://y");
    await state().send("t", loc);

    const [id, l, content] = vi.mocked(api.sendRequestOutcome).mock.calls[0];
    expect(id).toMatch(/^[0-9a-f-]{36}$/);
    expect(l).toBe(loc);
    expect(content.url).toBe("http://y");
    expect(state().byTab.t).toMatchObject({ sending: false, response: meta });
  });

  it("keeps script reports, and a null meta leaves no response", async () => {
    const report = { phase: "pre_request", console: [], console_truncated: false, tests: [], changes: [], error: { kind: "exception", message: "boom", script: "request", line: 2 } };
    vi.mocked(api.sendRequestOutcome).mockResolvedValue({ meta: null, scripts: [report], error: null } as never);
    await state().load("t", loc);
    await state().send("t", loc);
    expect(state().byTab.t).toMatchObject({ response: null, error: null, scripts: [report] });

    vi.mocked(api.sendRequestOutcome).mockRejectedValue({ code: "network", message: "x" });
    await state().send("t", loc);
    expect(state().byTab.t.scripts).toEqual([]);
  });

  it("keeps the pre-request report next to a transport failure", async () => {
    const report = { phase: "pre_request", console: [{ level: "log", text: "hi" }], console_truncated: false, tests: [], changes: [], error: null };
    const error = { code: "network", message: "connection refused" };
    vi.mocked(api.sendRequestOutcome).mockResolvedValue({ meta: null, scripts: [report], error } as never);
    await state().load("t", loc);
    await state().send("t", loc);
    expect(state().byTab.t).toMatchObject({ response: null, error, scripts: [report], sending: false });
  });

  it("cancel aborts the in-flight id and a rejection becomes the error", async () => {
    let reject: (e: unknown) => void = () => {};
    vi.mocked(api.sendRequestOutcome).mockReturnValue(new Promise((_, r) => (reject = r)));
    vi.mocked(api.cancelRequest).mockResolvedValue(null);
    await state().load("t", loc);
    const pending = state().send("t", loc);
    state().cancel("t");
    const id = vi.mocked(api.cancelRequest).mock.calls[0][0];
    expect(id).toBe(vi.mocked(api.sendRequestOutcome).mock.calls[0][0]);
    reject({ code: "cancelled", message: "Cancelled" });
    await pending;

    expect(state().byTab.t.error?.code).toBe("cancelled");
    expect(state().byTab.t.sending).toBe(false);
  });

  it("ignores a late result after the tab was dropped", async () => {
    let resolve: (m: never) => void = () => {};
    vi.mocked(api.sendRequestOutcome).mockReturnValue(new Promise((r) => (resolve = r)));
    vi.mocked(api.cancelRequest).mockResolvedValue(null);
    await state().load("t", loc);
    const pending = state().send("t", loc);
    state().drop("t");
    resolve({ meta, scripts: [], error: null } as never);
    await pending;

    expect(state().byTab.t).toBeUndefined();
  });
});
