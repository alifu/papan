import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { Page } from "@playwright/test";

const FIXTURE = join(dirname(fileURLToPath(import.meta.url)), "fixtures", "curl.json");

/**
 * In-memory fake of commands 38-46 (network settings, certificates, cookies, cURL). The proxy password
 * is write-only, as in the real backend. `import_curl` answers from `fixtures/curl.json`; `export_curl`
 * returns a fixed command. Must run after the base fake has published `window.__handlers`.
 */
export async function installNetFake(page: Page): Promise<void> {
  const cases = JSON.parse(readFileSync(FIXTURE, "utf8")) as unknown;
  await page.addInitScript((cases) => {
    type Args = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
    interface Cookie { name: string; value: string; domain: string; path: string; expires_at_ms: number | null; http_only: boolean; secure: boolean }
    interface Case { command: string; content: unknown; warnings: string[] }
    const w = window as unknown as { __handlers: Record<string, (a: Args) => unknown> };
    const appError = (code: string, message: string) => ({ code, message });

    let proxy = { mode: "system", url: "", username: "", has_password: false, bypass: [] as string[] };
    let certificates: unknown[] = [];
    let jar: Cookie[] = [];
    const settings = () => ({ proxy, certificates, storage: { mode: "keychain", message: null } });
    const sorted = () => [...jar].sort((a, b) => a.domain.localeCompare(b.domain) || a.name.localeCompare(b.name));
    const sameCookie = (a: Cookie, b: { domain: string; path: string | null; name: string | null }) =>
      a.domain === b.domain && (b.path === null || a.path === b.path) && (b.name === null || a.name === b.name);

    const handlers = w.__handlers;
    const send = handlers.send_request;
    Object.assign(handlers, {
      read_network_settings: settings,
      read_proxy_mode: () => proxy.mode,
      write_proxy_settings: (a: Args) => {
        const p = a.proxy;
        const has = p.password.op === "set" ? true : p.password.op === "clear" ? false : proxy.has_password;
        proxy = { mode: p.mode, url: p.url, username: p.username, has_password: has, bypass: p.bypass };
        return settings();
      },
      add_client_certificate: () => null,
      remove_client_certificate: (a: Args) => {
        certificates = certificates.filter((c) => (c as { id: string }).id !== a.id);
        return settings();
      },
      list_cookies: sorted,
      set_cookie: (a: Args) => {
        const c = a.cookie as Cookie;
        jar = [...jar.filter((x) => !(x.domain === c.domain && x.path === c.path && x.name === c.name)), c];
        return sorted();
      },
      delete_cookies: (a: Args) => {
        jar = jar.filter((c) => !sameCookie(c, a.filter));
        return sorted();
      },
      clear_cookies: () => {
        jar = [];
        return null;
      },
      export_curl: (a: Args) => `curl -X ${a.content.method} '${a.content.url}'`,
      import_curl: (a: Args) => {
        const text = String(a.text).trim();
        const found = (cases as Case[]).find((c) => c.command === text);
        if (!found?.content) throw appError("invalid_input", "That is not a cURL command.");
        return { content: found.content, warnings: found.warnings };
      },
      // The base fake has no timing; add a fixed one.
      send_request: async (a: Args) => ({
        ...((await send(a)) as object),
        timing: { dns_ms: 1, connect_ms: 2, wait_ms: 7, download_ms: 2, reused: false },
      }),
    });
  }, cases);
}
