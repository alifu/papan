const INDENT = "  ";

function isWellFormed(text: string): boolean {
  const doc = new DOMParser().parseFromString(text, "application/xml");
  return doc.getElementsByTagName("parsererror").length === 0;
}

/**
 * Re-indents well-formed XML, one tag per line; elements holding only text stay on one line.
 * Input that does not parse (for example one with `{{variables}}` in tag names) is returned as is.
 */
export function formatXmlText(text: string): string {
  const source = text.trim();
  if (source === "" || !isWellFormed(source)) return text;
  const lines: string[] = [];
  let depth = 0;
  for (const token of source.split(/(?<=>)\s*(?=<)/)) {
    const closing = token.startsWith("</");
    if (closing) depth = Math.max(0, depth - 1);
    lines.push(`${INDENT.repeat(depth)}${token.trim()}`);
    const opening = /^<[^!?/]/.test(token) && !/\/>$/.test(token) && !/<\/[^>]*>$/.test(token);
    if (opening) depth += 1;
  }
  return lines.join("\n");
}
