import type { RequestContent, WsMessage, WsSettings, WsTemplate } from "../../ipc/bindings";

/** Same cap as the Rust log (plan 07 §5). */
export const LOG_CAP = 1000;
export const MAX_SUBPROTOCOLS = 20;
export const MAX_SUBPROTOCOL_LENGTH = 128;
export const MAX_TEMPLATE_NAME = 256;
const PREVIEW_LENGTH = 120;
const TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/;

/** Adds messages not yet logged (by `seq`), keeps `seq` order and the newest LOG_CAP. */
export function mergeMessages(log: readonly WsMessage[], incoming: readonly WsMessage[]): readonly WsMessage[] {
  const seen = new Set(log.map((m) => m.seq));
  const fresh = incoming.filter((m) => !seen.has(m.seq));
  if (fresh.length === 0) return log;
  return [...log, ...fresh].sort((a, b) => a.seq - b.seq).slice(-LOG_CAP);
}

const pad = (n: number, width = 2): string => String(n).padStart(width, "0");

export function formatTime(atMs: number | null): string {
  if (atMs === null) return "";
  const d = new Date(atMs);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
}

export function previewOf(message: WsMessage): string {
  const flat = message.text.replace(/\s+/g, " ").trim();
  return flat.length > PREVIEW_LENGTH ? `${flat.slice(0, PREVIEW_LENGTH)}…` : flat;
}

/** Pretty JSON for text frames that parse; everything else unchanged. */
export function expandedText(message: WsMessage): string {
  if (message.frame !== "text") return message.text;
  try {
    return JSON.stringify(JSON.parse(message.text), null, 2);
  } catch {
    return message.text;
  }
}

const hasVariable = (text: string): boolean => text.includes("{{");

export function jsonWarning(text: string): string | null {
  if (text.trim() === "" || hasVariable(text)) return null;
  try {
    JSON.parse(text);
    return null;
  } catch {
    return "This is not valid JSON.";
  }
}

export function base64Error(text: string): string | null {
  const compact = text.replace(/\s+/g, "");
  if (compact === "" || hasVariable(compact)) return null;
  return compact.length % 4 === 0 && BASE64.test(compact) ? null : "Binary messages must be valid base64.";
}

export function subprotocolError(value: string, existing: readonly string[]): string | null {
  if (value === "") return "A subprotocol cannot be empty.";
  if (value.length > MAX_SUBPROTOCOL_LENGTH) return `A subprotocol is at most ${MAX_SUBPROTOCOL_LENGTH} characters.`;
  if (!TOKEN.test(value)) return "A subprotocol must be an HTTP token (letters, digits and !#$%&'*+-.^_`|~).";
  if (existing.includes(value)) return "That subprotocol is already listed.";
  if (existing.length >= MAX_SUBPROTOCOLS) return `At most ${MAX_SUBPROTOCOLS} subprotocols are allowed.`;
  return null;
}

export function templateNameError(name: string, others: readonly string[]): string | null {
  if (name.trim() === "") return "A template name cannot be empty.";
  if (name.length > MAX_TEMPLATE_NAME) return `A template name is at most ${MAX_TEMPLATE_NAME} characters.`;
  if (others.includes(name)) return "A template with that name already exists.";
  return null;
}

const NO_WS: WsSettings = { subprotocols: [], messages: [] };

/** The `ws` section, which older content objects may lack. */
export const wsSettingsOf = (content: RequestContent): WsSettings => content.ws ?? NO_WS;

/** Adds `template`, or replaces the one with the same name. */
export function upsertTemplate(templates: readonly WsTemplate[], template: WsTemplate): WsTemplate[] {
  return templates.some((t) => t.name === template.name)
    ? templates.map((t) => (t.name === template.name ? template : t))
    : [...templates, template];
}
