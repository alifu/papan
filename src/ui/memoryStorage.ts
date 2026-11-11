import { vi } from "vitest";

/**
 * Test helper: replaces `localStorage` with an in-memory Storage.
 * Node's built-in `localStorage` global shadows jsdom's and is unusable without a file.
 * Callers `vi.unstubAllGlobals()` in `afterEach`.
 */
export function installMemoryStorage(): Storage {
  const data = new Map<string, string>();
  const storage: Storage = {
    get length() {
      return data.size;
    },
    clear: () => data.clear(),
    getItem: (key) => data.get(key) ?? null,
    key: (index) => Array.from(data.keys())[index] ?? null,
    removeItem: (key) => {
      data.delete(key);
    },
    setItem: (key, value) => {
      data.set(key, String(value));
    },
  };
  vi.stubGlobal("localStorage", storage);
  return storage;
}
