import { describe, expect, it } from "vitest";
import type { StoredCookie } from "../../ipc/bindings";
import { cookieRows, formatExpiry, formatFlags } from "./cookieRows";

const c = (over: Partial<StoredCookie>): StoredCookie => ({
  name: "a", value: "1", domain: "a.test", path: "/", expires_at_ms: null, http_only: false, secure: false, ...over,
});

describe("cookieRows", () => {
  it("groups by domain and sorts by domain, path, name", () => {
    const input = [c({ domain: "b.test" }), c({ name: "z" }), c({ name: "y", path: "/a" }), c({ name: "m" })];
    const groups = cookieRows(input);
    expect(groups.map((g) => g.domain)).toEqual(["a.test", "b.test"]);
    expect(groups[0].rows.map((r) => r.cookie.name)).toEqual(["m", "z", "y"]);
    expect(input[0].domain).toBe("b.test");
  });

  it("filters case-insensitively on name, domain and path", () => {
    const input = [c({ name: "Session" }), c({ name: "x", domain: "other.test" }), c({ name: "y", path: "/api" })];
    expect(cookieRows(input, " SESS ")[0].rows).toHaveLength(1);
    expect(cookieRows(input, "other")[0].domain).toBe("other.test");
    expect(cookieRows(input, "/api")[0].rows[0].cookie.name).toBe("y");
    expect(cookieRows(input, "nothing")).toEqual([]);
  });

  it("formats expiry and flags", () => {
    expect(formatExpiry(null)).toBe("Session");
    expect(formatExpiry(Date.UTC(2030, 0, 2, 3, 4, 5))).toBe("2030-01-02 03:04:05Z");
    expect(formatFlags({ http_only: true, secure: true })).toBe("HttpOnly, Secure");
    expect(formatFlags({ http_only: false, secure: false })).toBe("");
    expect(cookieRows([c({ secure: true })])[0].rows[0].flags).toBe("Secure");
  });
});
