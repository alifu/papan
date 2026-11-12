import type { Page } from "@playwright/test";

declare global {
  interface Window {
    /** Makes the next call to `cmd` reject with an AppError. */
    __failNext: (cmd: string) => void;
  }
}

/**
 * The browser has no Tauri runtime. This installs an in-memory fake of the backend
 * commands (app_info plus the 17 storage commands), seeded with one collection and one
 * scratch request, and stubs for the event plugin.
 */
export async function mockTauri(page: Page): Promise<void> {
  await page.addInitScript(() => {
    type Args = Record<string, unknown>;
    interface FakeNode { path: string; kind: "folder" | "request"; name: string; method: string; url: string }
    interface FakeCollection { id: string; name: string; nodes: FakeNode[] }
    interface FakeScratch { id: string; name: string; method: string; url: string }

    const request = (path: string, name: string, method: string, url: string): FakeNode =>
      ({ path, kind: "request", name, method, url });
    const collections: FakeCollection[] = [
      {
        id: "c1",
        name: "Demo API",
        nodes: [
          request("health.http.yaml", "Health", "GET", "https://api.example.com/health"),
          { path: "auth", kind: "folder", name: "Auth", method: "", url: "" },
          request("auth/login.http.yaml", "Login", "POST", "https://api.example.com/login"),
        ],
      },
    ];
    let scratch: FakeScratch[] = [{ id: "s1", name: "Quick check", method: "GET", url: "" }];
    let counter = 1;
    const failing = new Set<string>();

    const appError = (code: string, message: string) => ({ code, message });
    const parentOf = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");
    const within = (prefix: string, path: string) => path === prefix || path.startsWith(`${prefix}/`);
    const slug = (name: string) =>
      name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "untitled";
    const find = (id: unknown): FakeCollection => {
      const found = collections.find((c) => c.id === id);
      if (!found) throw appError("not_found", "That collection is not open.");
      return found;
    };
    const node = (c: FakeCollection, path: string): FakeNode => {
      const found = c.nodes.find((n) => n.path === path);
      if (!found) throw appError("not_found", "That item no longer exists.");
      return found;
    };
    const childrenOf = (c: FakeCollection, parent: string): unknown[] =>
      c.nodes
        .filter((n) => parentOf(n.path) === parent)
        .map((n) => ({
          path: n.path,
          name: n.name,
          kind: n.kind,
          method: n.kind === "request" ? n.method : null,
          children: n.kind === "folder" ? childrenOf(c, n.path) : [],
        }));
    const tree = (c: FakeCollection) => ({
      collection: { id: c.id, name: c.name, path: `/demo/${c.id}`, available: true },
      children: childrenOf(c, ""),
    });
    const change = (c: FakeCollection, path: string | null) => ({ tree: tree(c), path });
    const newPath = (c: FakeCollection, parent: string, name: string, isRequest: boolean): string => {
      const taken = new Set(c.nodes.map((n) => n.path.toLowerCase()));
      const suffix = isRequest ? ".http.yaml" : "";
      for (let i = 1; ; i++) {
        const stem = i === 1 ? slug(name) : `${slug(name)}-${i}`;
        const path = `${parent ? `${parent}/` : ""}${stem}${suffix}`;
        if (!taken.has(path.toLowerCase())) return path;
      }
    };
    const requireName = (name: unknown): string => {
      const trimmed = String(name).trim();
      if (trimmed === "") throw appError("invalid_input", "Name is required.");
      return trimmed;
    };
    const target = (location: { scope: string; id?: string; collection_id?: string; path?: string }) =>
      location.scope === "scratch"
        ? scratch.find((s) => s.id === location.id) ??
          (() => { throw appError("not_found", "That request no longer exists."); })()
        : node(find(location.collection_id), String(location.path));
    const detach = (c: FakeCollection, path: string): FakeNode[] => {
      const moved = c.nodes.filter((n) => within(path, n.path));
      c.nodes = c.nodes.filter((n) => !within(path, n.path));
      return moved;
    };
    const attach = (c: FakeCollection, moved: FakeNode[], from: string, to: string, folder: string, index: number) => {
      const rebased = moved.map((n) => ({ ...n, path: `${to}${n.path.slice(from.length)}` }));
      const siblings = c.nodes.filter((n) => parentOf(n.path) === folder);
      const before = siblings[Math.min(index, siblings.length)];
      const at = before ? c.nodes.indexOf(before) : c.nodes.length;
      c.nodes.splice(at, 0, ...rebased);
    };
    const scratchList = () => scratch.map(({ id, name, method }) => ({ id, name, method }));

    const handlers: Record<string, (a: Args) => unknown> = {
      app_info: () => ({ name: "Papan", version: "0.0.0-e2e" }),
      list_collections: () => collections.map(tree),
      create_collection: (a) => {
        const c: FakeCollection = { id: `c${++counter}`, name: requireName(a.name), nodes: [] };
        collections.push(c);
        return tree(c);
      },
      open_collection: () => null,
      close_collection: (a) => {
        collections.splice(collections.indexOf(find(a.collectionId)), 1);
        return null;
      },
      read_tree: (a) => tree(find(a.collectionId)),
      create_folder: (a) => {
        const c = find(a.collectionId);
        const name = requireName(a.name);
        const path = newPath(c, String(a.parent), name, false);
        c.nodes.push({ path, kind: "folder", name, method: "", url: "" });
        return change(c, path);
      },
      create_request: (a) => {
        const c = find(a.collectionId);
        const name = requireName(a.name);
        const path = newPath(c, String(a.parent), name, true);
        c.nodes.push(request(path, name, "GET", ""));
        return change(c, path);
      },
      rename_node: (a) => {
        const c = find(a.collectionId);
        const target = node(c, String(a.path));
        target.name = requireName(a.name);
        return change(c, target.path);
      },
      delete_node: (a) => {
        const c = find(a.collectionId);
        node(c, String(a.path));
        detach(c, String(a.path));
        return change(c, null);
      },
      move_node: (a) => {
        const c = find(a.collectionId);
        const path = String(a.path);
        const folder = String(a.targetFolder);
        node(c, path);
        if (within(path, folder)) throw appError("invalid_input", "A folder cannot move into itself.");
        const to = newPath(c, folder, node(c, path).name, node(c, path).kind === "request");
        const moved = detach(c, path);
        attach(c, moved, path, to, folder, Number(a.index));
        return change(c, to);
      },
      read_request: (a) => {
        const t = target(a.location as never);
        return { name: t.name, method: t.method, url: t.url };
      },
      write_request: (a) => {
        const t = target(a.location as never);
        const content = a.content as { method: string; url: string };
        t.method = content.method;
        t.url = content.url;
        return null;
      },
      list_scratch: scratchList,
      create_scratch: () => {
        const item = { id: `s${++counter}`, name: "Untitled", method: "GET", url: "" };
        scratch.push(item);
        return { id: item.id, name: item.name, method: item.method };
      },
      rename_scratch: (a) => {
        (target({ scope: "scratch", id: String(a.id) }) as FakeScratch).name = requireName(a.name);
        return scratchList();
      },
      delete_scratch: (a) => {
        scratch = scratch.filter((s) => s.id !== a.id);
        return scratchList();
      },
      move_scratch_to_collection: (a) => {
        const c = find(a.collectionId);
        const item = target({ scope: "scratch", id: String(a.id) }) as FakeScratch;
        const folder = String(a.targetFolder);
        const path = newPath(c, folder, item.name, true);
        c.nodes.push(request(path, item.name, item.method, item.url));
        scratch = scratch.filter((s) => s.id !== item.id);
        return change(c, path);
      },
    };

    window.__failNext = (cmd) => void failing.add(cmd);

    let callbackId = 0;
    const internals = {
      transformCallback: () => ++callbackId,
      unregisterCallback: () => {},
      invoke: (cmd: string, args: Args = {}) => {
        if (cmd.startsWith("plugin:event|")) return Promise.resolve(++callbackId);
        if (failing.delete(cmd)) return Promise.reject(appError("io", "Backend says no."));
        const handler = handlers[cmd];
        if (!handler) return Promise.reject(new Error(`unmocked command: ${cmd}`));
        try {
          return Promise.resolve(handler(args));
        } catch (err) {
          return Promise.reject(err);
        }
      },
    };
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = internals;
    (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {},
    };
  });
}
