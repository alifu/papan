import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { Page } from "@playwright/test";

const FIXTURE = join(dirname(fileURLToPath(import.meta.url)), "fixtures", "environments.json");

/**
 * In-memory fake of commands 27-37 (environments, variables, secrets). Secret values are write-only,
 * as in the real backend. It also wraps `send_request` to substitute `{{name}}` in the URL and report
 * what it could not resolve. Must run after the base fake has published `window.__handlers`.
 */
export async function installEnvFake(page: Page): Promise<void> {
  const seed = JSON.parse(readFileSync(FIXTURE, "utf8")) as unknown;
  await page.addInitScript((seed) => {
    type Args = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
    interface Entry { value: string; current: string | null; enabled: boolean; secret: boolean; stored: string | null }
    interface Env { env: string; name: string; vars: Map<string, Entry> }
    interface Fixture {
      globals: { variables: Record<string, { value?: string; secret?: boolean }> };
      environments: { env: string; name: string; variables: Record<string, { value?: string; secret?: boolean }> }[];
      selected_env: string | null;
    }
    const fixture = seed as Fixture;
    const w = window as unknown as { __handlers: Record<string, (a: Args) => unknown> };
    const appError = (code: string, message: string) => ({ code, message });
    const toVars = (src: Record<string, { value?: string; secret?: boolean }>): Map<string, Entry> =>
      new Map(Object.entries(src).map(([name, v]) => [name, { value: v.value ?? "", current: null, enabled: true, secret: !!v.secret, stored: null }]));

    const globals = toVars(fixture.globals.variables);
    const collectionVars = new Map<string, Map<string, Entry>>();
    const envs = new Map<string, Env[]>([["c1", fixture.environments.map((e) => ({ env: e.env, name: e.name, vars: toVars(e.variables) }))]]);
    const selected = new Map<string, string | null>([["c1", fixture.selected_env]]);

    const envsOf = (cid: string): Env[] => envs.get(cid) ?? [];
    const list = (cid: string) => ({
      environments: [...envsOf(cid)]
        .sort((a, b) => a.name.localeCompare(b.name) || a.env.localeCompare(b.env))
        .map((e) => ({ env: e.env, name: e.name, invalid: false })),
      selected: selected.get(cid) ?? null,
    });
    const scopeVars = (scope: { scope: string; collection_id?: string; env?: string }): Map<string, Entry> => {
      if (scope.scope === "global") return globals;
      const cid = String(scope.collection_id);
      if (scope.scope === "collection") {
        if (!collectionVars.has(cid)) collectionVars.set(cid, new Map());
        return collectionVars.get(cid) as Map<string, Entry>;
      }
      const found = envsOf(cid).find((e) => e.env === scope.env);
      if (!found) throw appError("not_found", "That environment no longer exists.");
      return found.vars;
    };
    const read = (vars: Map<string, Entry>) =>
      [...vars.entries()]
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([name, v]) => ({
          name,
          value: v.secret ? "" : v.value,
          current: v.secret ? null : v.current,
          enabled: v.enabled,
          secret: v.secret,
          has_secret: v.secret && v.stored !== null,
        }));

    const DYNAMIC: Record<string, () => string> = {
      $uuid: () => "00000000-0000-4000-8000-000000000000",
      $timestamp: () => String(Math.floor(Date.now() / 1000)),
      $isoTimestamp: () => new Date().toISOString(),
      $randomInt: () => String(Math.floor(Math.random() * 1001)),
    };
    const layers = (location: { scope: string; collection_id?: string }): [string, Map<string, Entry>][] => {
      const out: [string, Map<string, Entry>][] = [["global", globals]];
      if (location.scope !== "collection") return out;
      const cid = String(location.collection_id);
      out.push(["collection", scopeVars({ scope: "collection", collection_id: cid })]);
      const sel = selected.get(cid);
      if (sel) out.push(["environment", scopeVars({ scope: "environment", collection_id: cid, env: sel })]);
      return out;
    };
    const effective = (location: { scope: string; collection_id?: string }) => {
      const merged = new Map<string, { value: string; source: string; secret: boolean; resolved: boolean }>();
      for (const [source, vars] of layers(location)) {
        for (const [name, v] of vars) {
          if (!v.enabled) continue;
          merged.set(name, { value: v.secret ? "" : v.value, source, secret: v.secret, resolved: !v.secret || v.stored !== null });
        }
      }
      for (const name of Object.keys(DYNAMIC)) merged.set(name, { value: "", source: "dynamic", secret: false, resolved: true });
      return [...merged.entries()].map(([name, v]) => ({ name, ...v }));
    };
    const TOKEN = /\{\{ *([^{}\r\n]+?) *\}\}/g;
    const substitute = (location: { scope: string; collection_id?: string }, text: string, mask: boolean) => {
      const known = new Map(effective(location).map((v) => [v.name, v]));
      const unresolved = new Set<string>();
      const out = text.replace(TOKEN, (whole, name: string) => {
        const v = known.get(name);
        if (!v || !v.resolved) {
          unresolved.add(name);
          return whole;
        }
        if (v.source === "dynamic") return mask ? whole : DYNAMIC[name]();
        if (v.secret) return mask ? "•••••" : "secret";
        return v.value;
      });
      return { text: out, unresolved: [...unresolved] };
    };

    const handlers = w.__handlers;
    const slug = (name: string) => name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "env";
    Object.assign(handlers, {
      list_environments: (a: Args) => list(a.collectionId),
      create_environment: (a: Args) => {
        const name = String(a.name).trim();
        if (!name) throw appError("invalid_input", "Name is required.");
        const taken = new Set(envsOf(a.collectionId).map((e) => e.env));
        let env = slug(name);
        for (let i = 2; taken.has(env); i++) env = `${slug(name)}-${i}`;
        envs.set(a.collectionId, [...envsOf(a.collectionId), { env, name, vars: new Map() }]);
        return list(a.collectionId);
      },
      rename_environment: (a: Args) => {
        const found = envsOf(a.collectionId).find((e) => e.env === a.env);
        if (found) found.name = String(a.name).trim();
        return list(a.collectionId);
      },
      delete_environment: (a: Args) => {
        envs.set(a.collectionId, envsOf(a.collectionId).filter((e) => e.env !== a.env));
        if (selected.get(a.collectionId) === a.env) selected.set(a.collectionId, null);
        return list(a.collectionId);
      },
      select_environment: (a: Args) => {
        selected.set(a.collectionId, a.env ?? null);
        return null;
      },
      read_variables: (a: Args) => read(scopeVars(a.scope)),
      write_variables: (a: Args) => {
        const vars = scopeVars(a.scope);
        const next = new Map<string, Entry>();
        for (const input of a.variables as Args[]) {
          if (next.has(input.name)) throw appError("invalid_input", `Duplicate variable ${input.name}.`);
          const before = vars.get(input.previous_name ?? input.name);
          let stored = input.secret ? (before?.stored ?? null) : null;
          if (input.secret_edit.op === "set") stored = input.secret_edit.value;
          if (input.secret_edit.op === "clear") stored = null;
          next.set(input.name, { value: input.value, current: input.current, enabled: input.enabled, secret: input.secret, stored });
        }
        vars.clear();
        for (const [k, v] of next) vars.set(k, v);
        return read(vars);
      },
      effective_variables: (a: Args) => effective(a.location),
      preview_template: (a: Args) => substitute(a.location, String(a.text), true),
      secret_storage_status: () => ({ mode: "keychain", message: null }),
      reset_secret_storage: () => ({ mode: "keychain", message: null }),
    });

    const baseSend = handlers.send_request as (a: Args) => Promise<Record<string, unknown>>;
    handlers.send_request = (a: Args) => {
      const content = a.content as { url: string };
      const { text, unresolved } = substitute(a.location, content.url, false);
      return baseSend({ ...a, content: { ...content, url: text } }).then((meta) => ({ ...meta, unresolved }));
    };
  }, seed);
}
