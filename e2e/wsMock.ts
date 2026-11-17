import type { Page } from "@playwright/test";

/**
 * In-memory fake of the M7 WebSocket commands (`ws_connect`, `ws_send`, `ws_disconnect`, `ws_log`, `ws_clear_log`)
 * plus the `ws` kind handling of `create_scratch` and `read_request` for scratch requests. A URL containing `refuse`
 * fails like a refused connection. The event plugin is a stub in the base fake, so this taps `transformCallback` and
 * `plugin:event|listen` itself and delivers `ws-event` payloads to the page's listeners; `window.__wsEmit(event)` pushes
 * one by hand, `window.__wsPush(text)` makes the last connection receive a message, and `ws_send` answers with an echo event. Must run after `mockTauri` (needs `window.__handlers`).
 */
export async function installWsFake(page: Page): Promise<void> {
  await page.addInitScript(() => {
    type Args = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
    interface Internals {
      transformCallback: (cb: (e: unknown) => void) => number;
      invoke: (cmd: string, args?: Args) => Promise<unknown>;
    }
    const w = window as unknown as {
      __handlers: Record<string, (a: Args) => any>; // eslint-disable-line @typescript-eslint/no-explicit-any
      __TAURI_INTERNALS__: Internals;
      __wsEmit: (payload: unknown) => void;
      __wsPush: (text: string) => void;
    };
    const handlers = w.__handlers;
    const internals = w.__TAURI_INTERNALS__;
    const appError = (code: string, message: string) => ({ code, message, detail: null });

    const FIRST_ID = 1_000_000;
    const callbacks = new Map<number, (e: unknown) => void>();
    const listeners = new Set<number>();
    const originalInvoke = internals.invoke.bind(internals);
    internals.transformCallback = (cb) => {
      const id = FIRST_ID + callbacks.size + 1;
      callbacks.set(id, cb);
      return id;
    };
    internals.invoke = (cmd, args = {}) => {
      if (cmd === "plugin:event|listen" && args.event === "ws-event") {
        listeners.add(Number(args.handler));
        return Promise.resolve(Number(args.handler));
      }
      return originalInvoke(cmd, args);
    };
    w.__wsEmit = (payload) => {
      for (const id of listeners) callbacks.get(id)?.({ event: "ws-event", id, payload });
    };

    let lastConnection = "";
    const kinds = new Set<string>();
    const logs = new Map<string, unknown[]>();
    const log = (id: string) => logs.get(id) ?? [];
    const push = (id: string, direction: "sent" | "received", frame: string, text: string) => {
      const entry = { seq: log(id).length + 1, at_ms: Date.now(), direction, frame, text, size: text.length, truncated: false };
      logs.set(id, [...log(id), entry]);
      return entry;
    };
    w.__wsPush = (text) => {
      const entry = push(lastConnection, "received", "text", text);
      w.__wsEmit({ connection_id: lastConnection, kind: { type: "messages", items: [entry] } });
    };
    const later = (fn: () => void) => setTimeout(fn, 0);
    const original = { create_scratch: handlers.create_scratch, read_request: handlers.read_request };

    Object.assign(handlers, {
      create_scratch: (a: Args) => {
        const item = original.create_scratch(a) as { id: string };
        if (a.kind === "ws") kinds.add(item.id);
        return a.kind === "ws" ? { ...item, method: "GET", kind: "ws" } : item;
      },
      read_request: (a: Args) => {
        const doc = original.read_request(a) as { content: Record<string, unknown> };
        if (a.location.scope !== "scratch" || !kinds.has(a.location.id)) return doc;
        const ws = (doc.content.ws as object | undefined) ?? { subprotocols: [], messages: [] };
        return { ...doc, content: { ...doc.content, kind: "ws", method: "GET", ws } };
      },
      ws_connect: (a: Args) => {
        if (String(a.content.url).includes("refuse")) throw appError("network", "Connection refused");
        lastConnection = String(a.connectionId);
        const subprotocols = (a.content.ws?.subprotocols ?? []) as string[];
        const opened = { connection_id: a.connectionId, url: a.content.url, protocol: subprotocols[0] ?? null, status: 101, headers: [] };
        later(() => w.__wsEmit({ connection_id: a.connectionId, kind: { type: "opened", ...opened } }));
        return opened;
      },
      ws_send: (a: Args) => {
        const id = String(a.connectionId);
        const sent = push(id, "sent", a.message.frame === "binary" ? "binary" : "text", a.message.text);
        later(() => {
          const echo = push(id, "received", "text", `echo: ${a.message.text}`);
          w.__wsEmit({ connection_id: id, kind: { type: "messages", items: [echo] } });
        });
        return sent;
      },
      ws_disconnect: (a: Args) => {
        later(() => w.__wsEmit({ connection_id: a.connectionId, kind: { type: "closed", code: a.code, reason: a.reason, by: "client", error: null } }));
        return null;
      },
      ws_log: (a: Args) => log(String(a.connectionId)),
      ws_clear_log: (a: Args) => {
        logs.set(String(a.connectionId), []);
        return null;
      },
    });
  });
}
