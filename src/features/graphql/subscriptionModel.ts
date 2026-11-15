import type { RequestContent, SubscriptionSettings, WsMessage } from "../../ipc/bindings";
import { formatTime } from "../ws/wsModel";

export type SubResultKind = "next" | "error" | "complete" | "unparsed";

export interface SubResult {
  readonly seq: number;
  readonly kind: SubResultKind;
  readonly time: string;
  readonly size: number;
  /** Pretty JSON of the payload; a note for a frame that cannot be shown. */
  readonly text: string;
  /** One-line summary for the collapsed row. */
  readonly preview: string;
}

const PREVIEW_LENGTH = 120;
const NOT_SHOWN = "This result is not shown (too large or not valid JSON).";
const SCHEME_MAP: Readonly<Record<string, string>> = { "http:": "ws:", "https:": "wss:" };

const flat = (text: string): string => {
  const one = text.replace(/\s+/g, " ").trim();
  return one.length > PREVIEW_LENGTH ? `${one.slice(0, PREVIEW_LENGTH)}…` : one;
};

/** Pretty JSON of a payload; text that does not parse is returned unchanged. */
export function pretty(payload: unknown): string {
  if (typeof payload === "string") {
    try {
      return JSON.stringify(JSON.parse(payload), null, 2);
    } catch {
      return payload;
    }
  }
  return JSON.stringify(payload, null, 2) ?? "";
}

/** The URL a subscription connects to: the override, else the request URL with http(s) turned into ws(s). */
export function wsUrlOf(url: string, override: string): string {
  const chosen = override.trim() !== "" ? override.trim() : url.trim();
  if (chosen === "") return "";
  for (const [from, to] of Object.entries(SCHEME_MAP)) {
    if (chosen.toLowerCase().startsWith(`${from}//`)) return `${to}${chosen.slice(from.length)}`;
  }
  return chosen;
}

const unparsed = (m: WsMessage): SubResult => ({
  seq: m.seq,
  kind: "unparsed",
  time: formatTime(m.at_ms),
  size: m.size,
  text: NOT_SHOWN,
  preview: NOT_SHOWN,
});

function parseFrame(text: string): { type: unknown; payload: unknown } | null {
  try {
    const value: unknown = JSON.parse(text);
    if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
    const record = value as Record<string, unknown>;
    return { type: record.type, payload: record.payload };
  } catch {
    return null;
  }
}

/**
 * Decodes a logged frame of a `graphql-transport-ws` connection into a result row.
 * Protocol housekeeping (init, ack, ping, pong, subscribe, close) and sent frames give `null`.
 */
export function resultOf(frame: WsMessage): SubResult | null {
  if (frame.direction !== "received" || frame.frame !== "text") return null;
  const parsed = frame.truncated ? null : parseFrame(frame.text);
  if (parsed === null) return unparsed(frame);
  const { type, payload } = parsed;
  if (type === "next" || type === "error") {
    const text = pretty(payload ?? null);
    return { seq: frame.seq, kind: type, time: formatTime(frame.at_ms), size: frame.size, text, preview: flat(text) };
  }
  if (type === "complete") {
    return { seq: frame.seq, kind: "complete", time: formatTime(frame.at_ms), size: frame.size, text: "", preview: "The server completed the subscription." };
  }
  return typeof type === "string" ? null : unparsed(frame);
}

/** Result rows of a log, in order. */
export const resultsOf = (log: readonly WsMessage[]): readonly SubResult[] =>
  log.flatMap((m) => {
    const r = resultOf(m);
    return r ? [r] : [];
  });

export interface SubscriptionStatusInput {
  readonly status: "idle" | "connecting" | "open" | "closing" | "closed";
  readonly closed: { readonly code: number | null; readonly reason: string; readonly by: "client" | "server" | "error" } | null;
}

const NORMAL_CLOSE = 1000;

/** The status pill text of a subscription tab. */
export function statusText(tab: SubscriptionStatusInput | undefined): string {
  const status = tab?.status ?? "idle";
  if (status === "idle") return "Not started";
  if (status === "connecting") return "Connecting";
  if (status === "open") return "Subscribed";
  if (status === "closing") return "Stopping";
  const closed = tab?.closed ?? null;
  if (closed === null || (closed.by === "client" && (closed.code === null || closed.code === NORMAL_CLOSE))) return "Stopped";
  if (closed.by === "server" && closed.code === NORMAL_CLOSE) return "Completed";
  const code = closed.code === null ? "" : ` (${closed.code})`;
  return `Ended${code}${closed.reason === "" ? "" : `: ${closed.reason}`}`;
}

/** Case-insensitive filter over the kind and the pretty payload. */
export const matchesFilter = (result: SubResult, text: string): boolean =>
  text === "" || `${result.kind} ${result.text} ${result.preview}`.toLowerCase().includes(text.toLowerCase());

const NO_SUBSCRIPTION: SubscriptionSettings = { url: "", init_payload: "" };

/** The `subscription` section of a request; a request without one reads as empty. */
export const subscriptionOf = (content: RequestContent): SubscriptionSettings => content.subscription ?? NO_SUBSCRIPTION;
