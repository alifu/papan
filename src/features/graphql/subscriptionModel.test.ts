import { describe, expect, it } from "vitest";
import type { WsMessage } from "../../ipc/bindings";
import { pretty, resultOf, resultsOf, statusText, wsUrlOf } from "./subscriptionModel";

const frame = (text: string, extra: Partial<WsMessage> = {}): WsMessage => ({
  seq: 1,
  at_ms: 0,
  direction: "received",
  frame: "text",
  text,
  size: text.length,
  truncated: false,
  ...extra,
});

describe("wsUrlOf", () => {
  it.each([
    ["https://h/graphql", "", "wss://h/graphql"],
    ["http://h/graphql", "", "ws://h/graphql"],
    ["HTTP://h/g", "", "ws://h/g"],
    ["ws://h/g", "", "ws://h/g"],
    ["https://h/graphql", "wss://x/ws", "wss://x/ws"],
    ["https://h/graphql", "{{ws}}", "{{ws}}"],
    ["", "", ""],
    ["{{base}}/graphql", "", "{{base}}/graphql"],
  ])("%s with override %s -> %s", (url, override, expected) => {
    expect(wsUrlOf(url, override)).toBe(expected);
  });
});

describe("resultOf", () => {
  it("decodes next into pretty JSON", () => {
    const r = resultOf(frame('{"id":"1","type":"next","payload":{"data":{"a":1}}}'));
    expect(r?.kind).toBe("next");
    expect(r?.text).toBe('{\n  "data": {\n    "a": 1\n  }\n}');
    expect(r?.preview).toBe('{ "data": { "a": 1 } }');
  });
  it("decodes error payloads", () => {
    const r = resultOf(frame('{"id":"1","type":"error","payload":[{"message":"boom"}]}'));
    expect(r?.kind).toBe("error");
    expect(r?.text).toContain("boom");
  });
  it("decodes complete as an end marker", () => {
    const r = resultOf(frame('{"id":"1","type":"complete"}'));
    expect(r).toMatchObject({ kind: "complete", text: "" });
  });
  it.each(["connection_init", "connection_ack", "ping", "pong", "subscribe"])("hides %s", (type) => {
    expect(resultOf(frame(JSON.stringify({ type })))).toBeNull();
  });
  it("hides sent frames and non-text frames", () => {
    expect(resultOf(frame('{"type":"next","payload":{}}', { direction: "sent" }))).toBeNull();
    expect(resultOf(frame("", { frame: "close" }))).toBeNull();
    expect(resultOf(frame("", { frame: "ping" }))).toBeNull();
  });
  it("shows a not-shown row for truncated, unparsable or typeless frames", () => {
    expect(resultOf(frame('{"type":"next"', { truncated: true }))?.kind).toBe("unparsed");
    expect(resultOf(frame("not json"))?.kind).toBe("unparsed");
    expect(resultOf(frame("[1]"))?.kind).toBe("unparsed");
    expect(resultOf(frame('{"payload":1}'))?.kind).toBe("unparsed");
  });
  it("shortens long previews", () => {
    const r = resultOf(frame(JSON.stringify({ type: "next", payload: { data: "x".repeat(300) } })));
    expect(r?.preview.length).toBeLessThanOrEqual(121);
    expect(r?.preview.endsWith("…")).toBe(true);
  });
});

describe("resultsOf", () => {
  it("keeps only result rows in order", () => {
    const log = [
      frame('{"type":"connection_ack"}', { seq: 1 }),
      frame('{"type":"next","payload":{"data":1}}', { seq: 2 }),
      frame('{"type":"complete","id":"1"}', { seq: 3 }),
    ];
    expect(resultsOf(log).map((r) => r.seq)).toEqual([2, 3]);
  });
});

describe("pretty", () => {
  it("indents objects and leaves non-JSON text", () => {
    expect(pretty({ a: 1 })).toBe('{\n  "a": 1\n}');
    expect(pretty("nope")).toBe("nope");
    expect(pretty('{"a":1}')).toBe('{\n  "a": 1\n}');
  });
});

describe("statusText", () => {
  const closed = (by: "client" | "server" | "error", code: number | null, reason = "") => ({ status: "closed" as const, closed: { by, code, reason } });
  it.each([
    [undefined, "Not started"],
    [{ status: "idle" as const, closed: null }, "Not started"],
    [{ status: "connecting" as const, closed: null }, "Connecting"],
    [{ status: "open" as const, closed: null }, "Subscribed"],
    [{ status: "closing" as const, closed: null }, "Stopping"],
    [{ status: "closed" as const, closed: null }, "Stopped"],
    [closed("client", 1000), "Stopped"],
    [closed("server", 1000), "Completed"],
    [closed("server", 4401, "Unauthorized"), "Ended (4401): Unauthorized"],
    [closed("error", null), "Ended"],
  ])("%j -> %s", (tab, expected) => {
    expect(statusText(tab)).toBe(expected);
  });
});
