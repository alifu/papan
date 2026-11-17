import type { Page } from "@playwright/test";

/**
 * In-memory fake of the M8 gRPC commands (`grpc_schema`, `grpc_reflect`, `grpc_import_proto`, `grpc_start`, `grpc_send`,
 * `grpc_end`, `grpc_cancel`, `grpc_log`, `grpc_clear_log`) plus the `grpc` kind handling of `create_scratch` and
 * `read_request` for scratch requests. The schema has `e2e.Greeter` with `Say` (unary), `Watch` (server stream),
 * `Collect` (client stream) and `Chat` (bidi). Unary finishes at once with `OK`; `Watch` streams until cancelled; `Chat`
 * echoes every `grpc_send` and finishes when `grpc_end` is called. A URL containing `refuse` fails like a refused connection.
 * The event plugin is a stub in the base fake, so this taps `transformCallback` and `plugin:event|listen` itself and
 * delivers `grpc-event` payloads. Must run after `mockTauri` (needs `window.__handlers`).
 */
export async function installGrpcFake(page: Page): Promise<void> {
  await page.addInitScript(() => {
    type Args = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
    interface Internals {
      transformCallback: (cb: (e: unknown) => void) => number;
      invoke: (cmd: string, args?: Args) => Promise<unknown>;
    }
    const w = window as unknown as {
      __handlers: Record<string, (a: Args) => any>; // eslint-disable-line @typescript-eslint/no-explicit-any
      __TAURI_INTERNALS__: Internals;
    };
    const handlers = w.__handlers;
    const internals = w.__TAURI_INTERNALS__;
    const appError = (code: string, message: string) => ({ code, message, detail: null });

    const FIRST_ID = 2_000_000;
    const callbacks = new Map<number, (e: unknown) => void>();
    const listeners = new Set<number>();
    const originalInvoke = internals.invoke.bind(internals);
    internals.transformCallback = (cb) => {
      const id = FIRST_ID + callbacks.size + 1;
      callbacks.set(id, cb);
      return id;
    };
    internals.invoke = (cmd, args = {}) => {
      if (cmd === "plugin:event|listen" && args.event === "grpc-event") {
        listeners.add(Number(args.handler));
        return Promise.resolve(Number(args.handler));
      }
      return originalInvoke(cmd, args);
    };
    const emit = (payload: unknown) => {
      for (const id of listeners) callbacks.get(id)?.({ event: "grpc-event", id, payload });
    };

    const method = (name: string, kind: string) => ({
      name,
      kind,
      input_type: `e2e.${name}Request`,
      output_type: `e2e.${name}Reply`,
      template: `{\n  "name": ""\n}`,
    });
    const schema = {
      source: "protos",
      services: [
        {
          full_name: "e2e.Greeter",
          methods: [method("Say", "unary"), method("Watch", "server_streaming"), method("Collect", "client_streaming"), method("Chat", "bidi")],
        },
      ],
      files: ["greeter.proto"],
      warnings: [],
      fetched_at_ms: null,
    };
    const status = (code: number, name: string, message = "") => ({ code, name, message, details_base64: null });

    const kinds = new Set<string>();
    const logs = new Map<string, unknown[]>();
    const attempts = new Map<string, number>();
    const log = (id: string) => logs.get(id) ?? [];
    const push = (id: string, direction: "sent" | "received", text: string) => {
      const entry = { seq: log(id).length + 1, at_ms: Date.now(), direction, frame: "text", text, size: text.length, truncated: false };
      logs.set(id, [...log(id), entry]);
      return entry;
    };
    const later = (fn: () => void) => setTimeout(fn, 0);
    const finish = (id: string, attempt: number, code: number, name: string, by: string) =>
      emit({
        call_id: id,
        attempt,
        kind: { type: "finished", status: status(code, name), trailers: [{ name: "x-trailer", value: "done", enabled: true }], by, elapsed_ms: 12, error: null },
      });
    const original = { create_scratch: handlers.create_scratch, read_request: handlers.read_request };
    const defaultGrpc = { source: "protos", protos: [], import_paths: [], service: "", method: "", message: "", deadline_ms: 0 };

    Object.assign(handlers, {
      create_scratch: (a: Args) => {
        const item = original.create_scratch(a) as { id: string };
        if (a.kind === "grpc") kinds.add(item.id);
        return a.kind === "grpc" ? { ...item, method: "POST", kind: "grpc" } : item;
      },
      read_request: (a: Args) => {
        const doc = original.read_request(a) as { content: Record<string, unknown> };
        if (a.location.scope !== "scratch" || !kinds.has(a.location.id)) return doc;
        const grpc = (doc.content.grpc as object | undefined) ?? defaultGrpc;
        return { ...doc, content: { ...doc.content, kind: "grpc", method: "POST", grpc } };
      },
      grpc_schema: () => schema,
      grpc_reflect: () => ({ ...schema, source: "reflection", fetched_at_ms: Date.now() }),
      grpc_import_proto: () => ({ added: ["greeter.proto"], warnings: [] }),
      grpc_start: (a: Args) => {
        const id = String(a.callId);
        if (String(a.content.url).includes("refuse")) throw appError("network", "Connection refused");
        const attempt = (attempts.get(id) ?? 0) + 1;
        attempts.set(id, attempt);
        logs.set(id, []);
        const name = String(a.content.grpc.method);
        const kind = schema.services[0]!.methods.find((m) => m.name === name)?.kind ?? "unary";
        const started = { call_id: id, attempt, path: `/e2e.Greeter/${name}`, kind, url: a.content.url };
        later(() => {
          emit({ call_id: id, attempt, kind: { type: "headers", metadata: [{ name: "x-served-by", value: "mock", enabled: true }] } });
          if (kind === "unary") {
            const reply = push(id, "received", `{"greeting":"hello"}`);
            emit({ call_id: id, attempt, kind: { type: "messages", items: [reply] } });
            finish(id, attempt, 0, "OK", "server");
          }
        });
        return started;
      },
      grpc_send: (a: Args) => {
        const id = String(a.callId);
        const sent = push(id, "sent", a.message);
        later(() => {
          const echo = push(id, "received", `{"echo":${JSON.stringify(a.message)}}`);
          emit({ call_id: id, attempt: attempts.get(id) ?? 1, kind: { type: "messages", items: [echo] } });
        });
        return sent;
      },
      grpc_end: (a: Args) => {
        const id = String(a.callId);
        later(() => finish(id, attempts.get(id) ?? 1, 0, "OK", "client"));
        return null;
      },
      grpc_cancel: (a: Args) => {
        const id = String(a.callId);
        later(() => finish(id, attempts.get(id) ?? 1, 1, "CANCELLED", "client"));
        return null;
      },
      grpc_log: (a: Args) => log(String(a.callId)),
      grpc_clear_log: (a: Args) => {
        logs.set(String(a.callId), []);
        return null;
      },
    });
  });
}
