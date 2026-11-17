import type { Page } from "@playwright/test";

declare global {
  interface Window {
    /** Pushes a Tauri event to the listeners the page registered (see `tauriMock.ts`). */
    __emit: (event: string, payload: unknown) => void;
    /** Stores a capture on a fake webhook listener and emits `webhook-captured`. */
    __webhookCapture: (listenerId: string, capture: { method: string; path: string; body: string }) => void;
  }
}

/**
 * In-memory fake of the M7 webhook commands (`list_webhooks`, `save_webhook`, `delete_webhook`, `start_webhook`,
 * `stop_webhook`, `list_captures`, `read_capture`, `clear_captures`, `replay_capture`). Starting a listener binds the
 * fake port 9123. `window.__webhookCapture` simulates an incoming request. Must run after the base fake has published
 * `window.__handlers`.
 */
export async function installWebhookFake(page: Page): Promise<void> {
  await page.addInitScript(() => {
    type Args = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
    interface FakeListener { config: Args; running: boolean; url: string | null; captured: number; unmatched: number }
    const w = window as unknown as { __handlers: Record<string, (a: Args) => any> }; // eslint-disable-line @typescript-eslint/no-explicit-any
    const handlers = w.__handlers;
    const appError = (code: string, message: string) => ({ code, message });

    let listeners: FakeListener[] = [];
    const captures = new Map<string, Args[]>();
    let counter = 0;
    const find = (id: string): FakeListener => {
      const found = listeners.find((l) => l.config.id === id);
      if (!found) throw appError("not_found", "That listener no longer exists.");
      return found;
    };
    const replace = (next: FakeListener): FakeListener => {
      listeners = listeners.map((l) => (l.config.id === next.config.id ? next : l));
      return next;
    };
    const summaryOf = (capture: Args) => capture.summary;

    window.__webhookCapture = (listenerId, input) => {
      const listener = find(listenerId);
      const capture = {
        summary: { id: `c${++counter}`, at_ms: Date.now(), method: input.method, path: input.path, size: input.body.length, content_type: "application/json" },
        query: "",
        headers: [{ name: "Content-Type", value: "application/json", enabled: true }, { name: "X-Signature", value: "sig-123", enabled: true }],
        remote: "127.0.0.1:50000",
        body_text: input.body,
        body_base64: null,
        truncated: false,
      };
      captures.set(listenerId, [...(captures.get(listenerId) ?? []), capture]);
      replace({ ...listener, captured: listener.captured + 1 });
      window.__emit("webhook-captured", { listener_id: listenerId, capture: summaryOf(capture) });
    };

    Object.assign(handlers, {
      list_webhooks: () => listeners,
      save_webhook: (a: Args) => {
        const config = a.config as Args;
        if (config.id === "") {
          const created = { config: { ...config, id: `w${++counter}` }, running: false, url: null, captured: 0, unmatched: 0 };
          listeners = [...listeners, created];
          return created;
        }
        const current = find(config.id);
        return replace({ ...current, config, url: current.running ? `http://127.0.0.1:9123${config.path}` : null });
      },
      delete_webhook: (a: Args) => {
        listeners = listeners.filter((l) => l.config.id !== a.id);
        return null;
      },
      start_webhook: (a: Args) => {
        const current = find(a.id);
        return replace({ ...current, running: true, url: `http://127.0.0.1:9123${current.config.path}` });
      },
      stop_webhook: (a: Args) => {
        replace({ ...find(a.id), running: false, url: null });
        return null;
      },
      list_captures: (a: Args) => (captures.get(a.id) ?? []).map(summaryOf),
      read_capture: (a: Args) => {
        const found = (captures.get(a.id) ?? []).find((c) => c.summary.id === a.captureId);
        if (!found) throw appError("not_found", "That capture is gone.");
        return found;
      },
      clear_captures: (a: Args) => {
        captures.set(a.id, []);
        replace({ ...find(a.id), captured: 0 });
        return null;
      },
      replay_capture: (a: Args) => {
        const found = (captures.get(a.id) ?? []).find((c) => c.summary.id === a.captureId);
        if (!found) throw appError("not_found", "That capture is gone.");
        const item = handlers.create_scratch({});
        const name = `${found.summary.method} ${found.summary.path} 12:04:05`;
        handlers.rename_scratch({ id: item.id, name });
        return { item: { ...item, name }, warnings: [] };
      },
    });
  });
}
