import type { Page } from "@playwright/test";

declare global {
  interface Window {
    /** Commands the Postman fake received, in order, with their arguments. */
    __postmanCalls: { cmd: string; args: Record<string, unknown> }[];
    /** Makes the next pick or export behave like the user cancelled the native dialog. */
    __postmanCancelNext: () => void;
  }
}

/**
 * In-memory fake of the M9 Postman commands (`postman_import_pick`, `postman_import_commit`, `postman_import_discard`,
 * `postman_export`). The pick answers one collection "Shop" (token `tok-1`, secret candidates `token` and `apiKey`, one
 * warning); commit adds "Shop" to `list_collections`; export reports two written files. Every call is recorded in
 * `window.__postmanCalls`. Must run after the base fake has published `window.__handlers`.
 */
export async function installPostmanFake(page: Page): Promise<void> {
  await page.addInitScript(() => {
    type Args = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
    const w = window as unknown as { __handlers: Record<string, (a: Args) => any> }; // eslint-disable-line @typescript-eslint/no-explicit-any
    const handlers = w.__handlers;
    const calls: Window["__postmanCalls"] = [];
    window.__postmanCalls = calls;
    let cancelNext = false;
    window.__postmanCancelNext = () => {
      cancelNext = true;
    };
    const takeCancel = () => {
      const was = cancelNext;
      cancelNext = false;
      return was;
    };
    const imported: unknown[] = [];
    const originalList = handlers.list_collections;
    handlers.list_collections = (a: Args) => [...originalList(a), ...imported];

    handlers.postman_import_pick = (a: Args) => {
      calls.push({ cmd: "postman_import_pick", args: a });
      if (takeCancel()) return null;
      return {
        token: "tok-1",
        files: [
          {
            file: "shop.postman_collection.json",
            kind: "collection_v21",
            name: "Shop",
            requests: 3,
            folders: 1,
            variables: 2,
            scripts: 2,
            secret_candidates: ["token", "apiKey"],
          },
        ],
        report: [{ level: "warning", file: "shop.postman_collection.json", path: "$.item[0].event[0]", message: "pm.cookies is not supported" }],
        truncated: 0,
      };
    };
    handlers.postman_import_commit = (a: Args) => {
      calls.push({ cmd: "postman_import_commit", args: a });
      imported.push({
        collection: { id: "c-shop", name: "Shop", path: "/tmp/shop", available: true },
        children: [],
      });
      return {
        collection_id: "c-shop",
        environments: [],
        globals: 0,
        report: [{ level: "info", file: "shop.postman_collection.json", path: "", message: "2 scripts imported but switched off; enable them per script after reading them." }],
      };
    };
    handlers.postman_import_discard = (a: Args) => {
      calls.push({ cmd: "postman_import_discard", args: a });
      return null;
    };
    handlers.postman_export = (a: Args) => {
      calls.push({ cmd: "postman_export", args: a });
      if (takeCancel()) return null;
      return {
        files: ["demo-api.postman_collection.json", "dev.postman_environment.json"],
        report: [{ level: "skipped", file: "demo-api", path: "auth/ws.ws.yaml", message: "WebSocket requests are not exported" }],
      };
    };
  });
}
