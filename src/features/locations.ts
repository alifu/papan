import type { RequestLocation } from "../ipc/bindings";

export type { RequestLocation };

export function scratchLocation(id: string): RequestLocation {
  return { scope: "scratch", id };
}

export function collectionLocation(collectionId: string, path: string): RequestLocation {
  return { scope: "collection", collection_id: collectionId, path };
}

export function sameLocation(a: RequestLocation, b: RequestLocation): boolean {
  if (a.scope === "scratch") return b.scope === "scratch" && a.id === b.id;
  return b.scope === "collection" && a.collection_id === b.collection_id && a.path === b.path;
}

/** True when `path` is `prefix` or lies below it. The empty prefix is the whole collection. */
export function isWithin(prefix: string, path: string): boolean {
  return prefix === "" || path === prefix || path.startsWith(`${prefix}/`);
}

/** True when `loc` is `target` or lies below it (collection scope only). */
export function locationWithin(target: RequestLocation, loc: RequestLocation): boolean {
  if (target.scope === "scratch") return sameLocation(target, loc);
  return (
    loc.scope === "collection" &&
    loc.collection_id === target.collection_id &&
    isWithin(target.path, loc.path)
  );
}

/** Maps `loc` from under `from` to the same relative place under `to`; null when not below `from`. */
export function relocate(
  from: RequestLocation,
  to: RequestLocation,
  loc: RequestLocation,
): RequestLocation | null {
  if (!locationWithin(from, loc)) return null;
  if (from.scope === "scratch" || to.scope === "scratch" || loc.scope === "scratch") return to;
  return { ...to, path: `${to.path}${loc.path.slice(from.path.length)}` };
}
