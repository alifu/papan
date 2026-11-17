import { describe, expect, it } from "vitest";
import type { Capture } from "../../ipc/bindings";
import { captureToCurl } from "./curl";

const capture = (patch: Partial<Capture> = {}): Capture => ({
  summary: { id: "c1", at_ms: 0, method: "POST", path: "/hooks/x", size: 3, content_type: "application/json" },
  query: "a=1",
  headers: [
    { name: "Host", value: "127.0.0.1:9000", enabled: true },
    { name: "Content-Type", value: "application/json", enabled: true },
    { name: "Content-Length", value: "3", enabled: true },
  ],
  remote: "127.0.0.1:5",
  body_text: "it's",
  body_base64: null,
  truncated: false,
  ...patch,
});

describe("captureToCurl", () => {
  it("rebuilds method, url with query, end-to-end headers and quoted body", () => {
    const text = captureToCurl(capture(), "http://127.0.0.1:9000/hooks");
    expect(text).toContain("curl -X 'POST' 'http://127.0.0.1:9000/hooks/x?a=1'");
    expect(text).toContain("-H 'Content-Type: application/json'");
    expect(text).not.toContain("Host:");
    expect(text).not.toContain("Content-Length");
    expect(text).toContain(`--data-raw 'it'\\''s'`);
  });

  it("omits the query and body when empty", () => {
    const text = captureToCurl(capture({ query: "", body_text: null }), "http://127.0.0.1:9000/hooks");
    expect(text).toContain("'http://127.0.0.1:9000/hooks/x'");
    expect(text).not.toContain("--data-raw");
  });

  it.each(["X&rm$IFS-rf$IFS~&", "GET;id", "P'O$(id)ST"])("shell-quotes the hostile method %s", (method) => {
    const base = capture();
    const text = captureToCurl(capture({ summary: { ...base.summary, method } }), "http://127.0.0.1:9000/hooks");
    const quoted = `'${method.replace(/'/g, `'\\''`)}'`;
    expect(text.startsWith(`curl -X ${quoted} `)).toBe(true);
  });
});
