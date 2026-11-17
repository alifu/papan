import type { StoredCookie } from "../../ipc/bindings";

export interface CookieRow {
  key: string;
  cookie: StoredCookie;
  expires: string;
  flags: string;
}

export interface CookieGroup {
  domain: string;
  rows: readonly CookieRow[];
}

export function formatExpiry(expiresAtMs: number | null): string {
  return expiresAtMs === null ? "Session" : new Date(expiresAtMs).toISOString().replace("T", " ").slice(0, 19) + "Z";
}

export function formatFlags(cookie: Pick<StoredCookie, "http_only" | "secure">): string {
  return [cookie.http_only ? "HttpOnly" : "", cookie.secure ? "Secure" : ""].filter(Boolean).join(", ");
}

function matches(cookie: StoredCookie, needle: string): boolean {
  return [cookie.name, cookie.domain, cookie.path].some((field) => field.toLowerCase().includes(needle));
}

/** Filters by name, domain or path (case-insensitive) and groups by domain, sorted by domain, path, name. */
export function cookieRows(cookies: readonly StoredCookie[], filter = ""): readonly CookieGroup[] {
  const needle = filter.trim().toLowerCase();
  const sorted = cookies
    .filter((cookie) => needle === "" || matches(cookie, needle))
    .sort((a, b) => a.domain.localeCompare(b.domain) || a.path.localeCompare(b.path) || a.name.localeCompare(b.name));
  const groups = new Map<string, CookieRow[]>();
  for (const cookie of sorted) {
    const row = {
      key: `${cookie.domain}\n${cookie.path}\n${cookie.name}`,
      cookie,
      expires: formatExpiry(cookie.expires_at_ms),
      flags: formatFlags(cookie),
    };
    groups.set(cookie.domain, [...(groups.get(cookie.domain) ?? []), row]);
  }
  return [...groups].map(([domain, rows]) => ({ domain, rows }));
}
