import { describe, expect, it } from "vitest";
import type { RequestContent, WsMessage } from "../../ipc/bindings";
import {
  LOG_CAP,
  base64Error,
  expandedText,
  formatTime,
  jsonWarning,
  mergeMessages,
  previewOf,
  subprotocolError,
  templateNameError,
  upsertTemplate,
  wsSettingsOf,
} from "./wsModel";

const msg = (seq: number, over: Partial<WsMessage> = {}): WsMessage => ({
  seq,
  at_ms: 0,
  direction: "received",
  frame: "text",
  text: `m${seq}`,
  size: 2,
  truncated: false,
  ...over,
});

describe("mergeMessages", () => {
  it("appends new messages in seq order without mutating", () => {
    const log = [msg(1)];
    const next = mergeMessages(log, [msg(3), msg(2)]);
    expect(next.map((m) => m.seq)).toEqual([1, 2, 3]);
    expect(log).toHaveLength(1);
  });

  it("ignores messages whose seq is already logged", () => {
    expect(mergeMessages([msg(1), msg(2)], [msg(2), msg(3)]).map((m) => m.seq)).toEqual([1, 2, 3]);
  });

  it("keeps only the newest LOG_CAP messages", () => {
    const many = Array.from({ length: LOG_CAP + 5 }, (_, i) => msg(i + 1));
    const next = mergeMessages([], many);
    expect(next).toHaveLength(LOG_CAP);
    expect(next[0]?.seq).toBe(6);
  });
});

describe("formatTime", () => {
  it("formats HH:mm:ss.SSS in local time", () => {
    const at = new Date(2026, 0, 2, 3, 4, 5, 6).getTime();
    expect(formatTime(at)).toBe("03:04:05.006");
  });
  it("is empty without a timestamp", () => {
    expect(formatTime(null)).toBe("");
  });
});

describe("previewOf and expandedText", () => {
  it("collapses whitespace and truncates", () => {
    expect(previewOf(msg(1, { text: "a\n  b" }))).toBe("a b");
    expect(previewOf(msg(1, { text: "x".repeat(300) })).endsWith("…")).toBe(true);
  });
  it("pretty prints JSON text frames", () => {
    expect(expandedText(msg(1, { text: '{"a":1}' }))).toBe('{\n  "a": 1\n}');
  });
  it("keeps plain text and base64 as is", () => {
    expect(expandedText(msg(1, { text: "hello" }))).toBe("hello");
    expect(expandedText(msg(1, { frame: "binary", text: "AAEC" }))).toBe("AAEC");
  });
  it("describes an empty control frame", () => {
    expect(previewOf(msg(1, { frame: "ping", text: "" }))).toBe("");
  });
});

describe("validators", () => {
  it("rejects invalid subprotocol tokens, duplicates and long values", () => {
    expect(subprotocolError("chat.v2", [])).toBeNull();
    expect(subprotocolError("a b", [])).toMatch(/token/);
    expect(subprotocolError("", [])).toMatch(/empty/i);
    expect(subprotocolError("chat", ["chat"])).toMatch(/already/);
    expect(subprotocolError("x".repeat(129), [])).toMatch(/128/);
  });
  it("rejects a 21st subprotocol", () => {
    const twenty = Array.from({ length: 20 }, (_, i) => `p${i}`);
    expect(subprotocolError("extra", twenty)).toMatch(/20/);
  });
  it("checks base64", () => {
    expect(base64Error("AAEC")).toBeNull();
    expect(base64Error("AA EC\n")).toBeNull();
    expect(base64Error("not base64!")).not.toBeNull();
    expect(base64Error("{{payload}}")).toBeNull();
    expect(base64Error("")).toBeNull();
  });
  it("warns about invalid JSON only without variables", () => {
    expect(jsonWarning('{"a":1}')).toBeNull();
    expect(jsonWarning("{oops")).toMatch(/JSON/);
    expect(jsonWarning('{"a": {{x}}}')).toBeNull();
    expect(jsonWarning("")).toBeNull();
  });
  it("validates template names", () => {
    expect(templateNameError("Join", ["Other"])).toBeNull();
    expect(templateNameError("  ", [])).toMatch(/empty/i);
    expect(templateNameError("Join", ["Join"])).toMatch(/already/);
    expect(templateNameError("x".repeat(257), [])).toMatch(/256/);
  });
});

describe("upsertTemplate", () => {
  const a = { name: "A", format: "text" as const, text: "1" };
  it("appends a new name without mutating", () => {
    const list = [a];
    expect(upsertTemplate(list, { ...a, name: "B" })).toHaveLength(2);
    expect(list).toHaveLength(1);
  });
  it("replaces the same name in place", () => {
    expect(upsertTemplate([a], { ...a, text: "2" })).toEqual([{ ...a, text: "2" }]);
  });
});

describe("wsSettingsOf", () => {
  it("defaults to empty settings", () => {
    expect(wsSettingsOf({} as RequestContent)).toEqual({ subprotocols: [], messages: [] });
  });
});
