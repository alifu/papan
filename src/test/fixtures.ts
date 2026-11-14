import type { CollectionTree, TreeNode } from "../ipc/bindings";

export const node = (
  path: string,
  kind: TreeNode["kind"],
  children: TreeNode[] = [],
  method: string | null = kind === "request" ? "GET" : null,
): TreeNode => ({
  path,
  name: path.split("/").pop()!.replace(".http.yaml", ""),
  kind,
  method,
  request_kind: kind === "request" ? "http" : null,
  children,
});

/** Collection "c1": health, auth/{login, logout}, broken (invalid). */
export const makeTree = (id = "c1", name = "My API", available = true): CollectionTree => ({
  collection: { id, name, path: `/tmp/${id}`, available },
  children: [
    node("health.http.yaml", "request"),
    node("auth", "folder", [
      node("auth/login.http.yaml", "request", [], "POST"),
      node("auth/logout.http.yaml", "request"),
    ]),
    node("broken.http.yaml", "invalid"),
  ],
});

export const appError = (message: string, code: "io" | "not_found" = "io") => ({ code, message });
