import type { Page } from "@playwright/test";

/** The browser has no Tauri runtime: answer `app_info` the way the backend would. */
export async function mockTauri(page: Page): Promise<void> {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      invoke: (cmd: string) =>
        cmd === "app_info"
          ? Promise.resolve({ name: "Papan", version: "0.0.0-e2e" })
          : Promise.reject(new Error(`unmocked command: ${cmd}`)),
    };
  });
}
