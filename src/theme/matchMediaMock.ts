import { vi } from "vitest";

type Listener = (event: { matches: boolean }) => void;

/** Installs a fake `window.matchMedia` for the dark-scheme query. */
export function installMatchMedia(initialDark: boolean): {
  setDark(dark: boolean): void;
  listenerCount(): number;
} {
  let dark = initialDark;
  const listeners = new Set<Listener>();
  vi.stubGlobal("matchMedia", (query: string) => ({
    get matches() {
      return dark;
    },
    media: query,
    addEventListener: (_type: string, listener: Listener) => {
      listeners.add(listener);
    },
    removeEventListener: (_type: string, listener: Listener) => {
      listeners.delete(listener);
    },
  }));
  return {
    setDark(next: boolean) {
      dark = next;
      listeners.forEach((listener) => listener({ matches: next }));
    },
    listenerCount: () => listeners.size,
  };
}
