import { readFileSync } from "node:fs";
import type { Page } from "@playwright/test";

const read = (name: string): string => readFileSync(`src/test/graphql/${name}`, "utf8");

declare global {
  interface Window {
    /** Contents of every request sent through the fake `send_request`, newest last. */
    __sent: Record<string, unknown>[];
    /** Contents of every request started through the fake `graphql_subscribe`, newest last. */
    __subscribed: Record<string, unknown>[];
  }
}

/**
 * In-memory fake of the M6 GraphQL commands plus the `kind` handling of `create_scratch`, `list_scratch`
 * and `read_request` for GraphQL scratch requests. `introspect_graphql` answers with the test schema
 * (introspection JSON), `load_schema_file` with its SDL, and a URL containing `down` fails. Runs after the
 * base fake and the SOAP fake have published their handlers.
 *
 * M6b: `graphql_subscribe` answers with `Opened`, then pushes a `connection_ack` and three `next` frames as `ws-event`
 * messages through `window.__wsEmit` (installed by `installWsFake`); a URL containing `refuse` fails like a rejected
 * start. `graphql_unsubscribe` pushes the `Closed` event. `window.__subscribed` holds the contents it was started with.
 */
export async function installGraphqlFake(page: Page): Promise<void> {
  const schemas = { introspection: read("introspection.json"), sdl: read("schema.graphql") };
  await page.addInitScript((schemas) => {
    type Args = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
    const w = window as unknown as {
      __handlers: Record<string, (a: Args) => any>; // eslint-disable-line @typescript-eslint/no-explicit-any
      __sent: Args[];
      __subscribed: Args[];
      __wsEmit: (payload: unknown) => void;
    };
    const handlers = w.__handlers;
    const appError = (code: string, message: string) => ({ code, message });
    w.__sent = [];
    w.__subscribed = [];
    const TICKS = 3;

    const graphqlIds = new Set<string>();
    let cached: { endpoint: string; fetched_at_ms: number | null; source: unknown } | null = null;
    const original = {
      create_scratch: handlers.create_scratch,
      list_scratch: handlers.list_scratch,
      read_request: handlers.read_request,
      send_request: handlers.send_request,
    };
    const tag = (item: { id: string }) => (graphqlIds.has(item.id) ? { ...item, kind: "graphql" } : item);

    Object.assign(handlers, {
      create_scratch: (a: Args) => {
        const item = original.create_scratch(a) as { id: string };
        if (a.kind === "graphql") graphqlIds.add(item.id);
        return tag(item);
      },
      list_scratch: () => (original.list_scratch({}) as { id: string }[]).map(tag),
      read_request: (a: Args) => {
        const doc = original.read_request(a) as { name: string; content: Record<string, unknown> };
        if (a.location.scope !== "scratch" || !graphqlIds.has(a.location.id) || doc.content.kind === "graphql") return doc;
        const body = { type: "graphql", query: "", variables: "", operation_name: "" };
        return { ...doc, content: { ...doc.content, kind: "graphql", method: "POST", body } };
      },
      send_request: (a: Args) => {
        w.__sent.push(a.content);
        return original.send_request(a);
      },
      graphql_subscribe: (a: Args) => {
        if (String(a.content.url).includes("refuse")) throw appError("network", "Connection refused");
        w.__subscribed.push(a.content);
        const id = String(a.connectionId);
        const opened = { connection_id: id, url: String(a.content.url).replace(/^http/, "ws"), protocol: "graphql-transport-ws", status: 101, headers: [] };
        const frame = (seq: number, body: unknown) => {
          const text = JSON.stringify(body);
          return { seq, at_ms: Date.now(), direction: "received", frame: "text", text, size: text.length, truncated: false };
        };
        setTimeout(() => {
          w.__wsEmit({ connection_id: id, kind: { type: "opened", ...opened } });
          const items = [frame(1, { type: "connection_ack" })];
          for (let n = 1; n <= TICKS; n += 1) items.push(frame(n + 1, { id, type: "next", payload: { data: { tick: n } } }));
          w.__wsEmit({ connection_id: id, kind: { type: "messages", items } });
        }, 0);
        return opened;
      },
      graphql_unsubscribe: (a: Args) => {
        setTimeout(() => w.__wsEmit({ connection_id: a.connectionId, kind: { type: "closed", code: 1000, reason: "", by: "client", error: null } }), 0);
        return null;
      },
      read_cached_schema: () => cached,
      introspect_graphql: (a: Args) => {
        if (String(a.content.url).includes("down")) throw appError("network", "Could not connect to localhost.");
        cached = { endpoint: "http://localhost/graphql", fetched_at_ms: Date.now(), source: { type: "introspection", json: schemas.introspection } };
        return cached;
      },
      load_schema_file: () => {
        cached = { endpoint: "http://localhost/graphql", fetched_at_ms: null, source: { type: "sdl", text: schemas.sdl } };
        return cached;
      },
      clear_cached_schema: () => {
        cached = null;
        return null;
      },
    });
  }, schemas);
}
