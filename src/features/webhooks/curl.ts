import type { Capture } from "../../ipc/bindings";

const quote = (text: string): string => `'${text.replace(/'/g, `'\\''`)}'`;
const SKIPPED_HEADERS = new Set(["host", "content-length", "connection", "transfer-encoding"]);

/** A cURL command that repeats a capture against its listener. */
export function captureToCurl(capture: Capture, listenerUrl: string): string {
  const origin = new URL(listenerUrl).origin;
  const query = capture.query === "" ? "" : `?${capture.query}`;
  const parts = [`curl -X ${quote(capture.summary.method)} ${quote(`${origin}${capture.summary.path}${query}`)}`];
  for (const header of capture.headers) {
    if (SKIPPED_HEADERS.has(header.name.toLowerCase())) continue;
    parts.push(`-H ${quote(`${header.name}: ${header.value}`)}`);
  }
  if (capture.body_text !== null && capture.body_text !== "") {
    parts.push(`--data-raw ${quote(capture.body_text)}`);
  }
  return parts.join(" \\\n  ");
}
