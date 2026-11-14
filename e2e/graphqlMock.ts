import { readFileSync } from "node:fs";
import type { Page } from "@playwright/test";

const read = (name: string): string => readFileSync(`src/test/graphql/${name}`, "utf8");

declare global {
  interface Window {
    /** Contents of every request sent through the fake `send_request`, newest last. */
    __sent: Record<string, unknown>[];
  }
}

/**
 * In-memory fake of the M6 GraphQL commands plus the `kind` handling of `create_scratch`, `list_scratch`
 * and `read_request` for GraphQL scratch requests. `introspect_graphql` answers with the test schema
 * (introspection JSON), `load_schema_file` with its SDL, and a URL containing `down` fails. Runs after the
 * base fake and the SOAP fake have published their handlers.
 */
export async function installGraphqlFake(page: Page): Promise<void> {
  const schemas = { introspection: read("introspection.json"), sdl: read("schema.graphql") };
  await page.addInitScript((schemas) => {
    type Args = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
    const w = window as unknown as { __handlers: Record<string, (a: Args) => any>; __sent: Args[] }; // eslint-disable-line @typescript-eslint/no-explicit-any
    const handlers = w.__handlers;
    const appError = (code: string, message: string) => ({ code, message });
    w.__sent = [];

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
