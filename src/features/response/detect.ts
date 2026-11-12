export type BodyKind = "json" | "xml" | "html" | "image" | "text";
export type BodyMode = "pretty" | "raw" | "tree" | "preview";

const mimeOf = (contentType: string | null | undefined): string =>
  (contentType ?? "").split(";")[0]?.trim().toLowerCase() ?? "";

/** Classifies a body by content type, sniffing the first character when the type is vague. */
export function detectKind(contentType: string | null | undefined, text = ""): BodyKind {
  const mime = mimeOf(contentType);
  if (mime.startsWith("image/")) return "image";
  if (mime === "application/json" || mime.endsWith("+json")) return "json";
  if (mime === "text/html" || mime === "application/xhtml+xml") return "html";
  if (mime === "application/xml" || mime === "text/xml" || mime.endsWith("+xml")) return "xml";
  if (mime === "" || mime === "text/plain") {
    const head = text.trimStart()[0];
    if (head === "{" || head === "[") return "json";
    if (head === "<" && /^<\?xml/i.test(text.trimStart())) return "xml";
  }
  return "text";
}

const MODES: Readonly<Record<BodyKind, readonly BodyMode[]>> = {
  json: ["pretty", "raw", "tree"],
  xml: ["pretty", "raw"],
  html: ["raw", "preview"],
  image: ["preview"],
  text: ["raw"],
};

export function availableModes(kind: BodyKind): readonly BodyMode[] {
  return MODES[kind];
}
