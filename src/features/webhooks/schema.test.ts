import { describe, expect, it } from "vitest";
import { DEFAULT_CONFIG, formFromConfig, validateForm, type ListenerForm } from "./schema";

const base = formFromConfig(DEFAULT_CONFIG);
const check = (patch: Partial<ListenerForm>) => validateForm("w1", { ...base, ...patch });
const errorsOf = (patch: Partial<ListenerForm>): readonly string[] => {
  const result = check(patch);
  return result.ok ? [] : result.errors;
};

describe("listener form validation", () => {
  it("accepts the defaults and builds a config with the id", () => {
    const result = check({});
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.config).toEqual({ ...DEFAULT_CONFIG, id: "w1" });
  });

  it.each([
    [{ name: "  " }, "Name is required."],
    [{ name: "x".repeat(65) }, "Name must be 64 characters or fewer."],
    [{ port: "80" }, "Port must be 0 (any free port) or 1024-65535."],
    [{ port: "70000" }, "Port must be 0 (any free port) or 1024-65535."],
    [{ port: "abc" }, "Port must be a number."],
    [{ path: "hooks" }, "Path must start with /."],
    [{ path: "/a/../b" }, "Path cannot contain .."],
    [{ path: "/a?b=1" }, "Path cannot contain ?, # or spaces."],
    [{ path: "/a b" }, "Path cannot contain ?, # or spaces."],
    [{ path: "/" + "a".repeat(256) }, "Path must be 256 characters or fewer."],
    [{ status: "199" }, "Status must be 200-599."],
    [{ status: "600" }, "Status must be 200-599."],
    [{ maxBodyBytes: "1000" }, "Body limit must be at least 1 KiB."],
    [{ maxBodyBytes: String(11 * 1024 * 1024) }, "Body limit must be 10 MiB or smaller."],
    [{ maxCaptures: "0" }, "Keep at least 1 capture."],
    [{ maxCaptures: "1001" }, "Keep at most 1000 captures."],
    [{ headers: [{ name: "Content-Length", value: "1" }] }, "Content-Length is set by the server."],
    [{ headers: [{ name: "Bad Name", value: "1" }] }, '"Bad Name" is not a valid header name.'],
    [{ headers: [{ name: "X-A", value: "a\nb" }] }, "Header values cannot contain line breaks."],
    [{ body: "x".repeat(1024 * 1024 + 1) }, "Response body must be 1 MiB or smaller."],
  ] as const)("rejects %j", (patch, message) => {
    expect(errorsOf(patch as Partial<ListenerForm>)).toContain(message);
  });

  it("allows port 0 and 1024", () => {
    expect(check({ port: "0" }).ok).toBe(true);
    expect(check({ port: "1024" }).ok).toBe(true);
  });

  it("rejects more than 50 response headers", () => {
    const headers = Array.from({ length: 51 }, (_, i) => ({ name: `X-${i}`, value: "v" }));
    expect(errorsOf({ headers })).toContain("At most 50 response headers.");
  });
});
