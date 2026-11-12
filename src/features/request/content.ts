import type { KeyValue, RequestContent, Settings } from "../../ipc/bindings";
import type { KeyValueRow } from "./KeyValueTable";

export const DEFAULT_SETTINGS: Settings = {
  follow_redirects: true,
  timeout_ms: 30000,
  verify_tls: true,
  http_version: "auto",
};

export function emptyContent(): RequestContent {
  return {
    method: "GET",
    url: "",
    disabled_params: [],
    headers: [],
    body: { type: "none" },
    auth: { type: "none" },
    settings: DEFAULT_SETTINGS,
  };
}

/** Structural equality for JSON-like values (key order does not matter). */
export function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const left = a as Record<string, unknown>;
  const right = b as Record<string, unknown>;
  const keys = Object.keys(left);
  if (keys.length !== Object.keys(right).length) return false;
  return keys.every((key) => key in right && deepEqual(left[key], right[key]));
}

export const equalContent = (a: RequestContent, b: RequestContent): boolean => deepEqual(a, b);

/** A copy of `content` with `patch` applied. */
export const withContent = (content: RequestContent, patch: Partial<RequestContent>): RequestContent => ({
  ...content,
  ...patch,
});

export const toRows = (items: readonly KeyValue[]): readonly KeyValueRow[] =>
  items.map((item, index) => ({ id: `kv-${index}`, ...item }));

const isBlank = (row: { name: string; value: string }): boolean => row.name === "" && row.value === "";

/** Rows back to stored entries; fully empty rows are dropped. */
export const fromRows = (rows: readonly KeyValueRow[]): KeyValue[] =>
  rows
    .filter((row) => !isBlank(row))
    .map(({ name, value, enabled }) => ({ name, value, enabled }));

export const countEnabled = (items: readonly KeyValue[]): number =>
  items.filter((item) => item.enabled && item.name !== "").length;
