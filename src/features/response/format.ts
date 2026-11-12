/** Pretty-printers for response bodies. Both return `null` when the input is not valid. */

const INDENT = "  ";

/**
 * Re-indents JSON by walking its tokens. Numbers are never parsed, so large
 * integers and duplicate keys survive. `JSON.parse` is used only to validate.
 */
export function prettyJson(text: string): string | null {
  try {
    JSON.parse(text);
  } catch {
    return null;
  }
  let out = "";
  let depth = 0;
  let i = 0;
  const newline = () => `\n${INDENT.repeat(depth)}`;
  const nextSignificant = (from: number): string => {
    let j = from;
    while (j < text.length && /\s/.test(text[j] ?? "")) j += 1;
    return text[j] ?? "";
  };
  while (i < text.length) {
    const ch = text[i] ?? "";
    if (ch === '"') {
      let j = i + 1;
      while (j < text.length && text[j] !== '"') j += text[j] === "\\" ? 2 : 1;
      out += text.slice(i, j + 1);
      i = j + 1;
    } else if (ch === "{" || ch === "[") {
      const close = ch === "{" ? "}" : "]";
      if (nextSignificant(i + 1) === close) {
        out += ch + close;
        i = text.indexOf(close, i + 1) + 1;
      } else {
        depth += 1;
        out += ch + newline();
        i += 1;
      }
    } else if (ch === "}" || ch === "]") {
      depth -= 1;
      out += newline() + ch;
      i += 1;
    } else if (ch === ",") {
      out += "," + newline();
      i += 1;
    } else if (ch === ":") {
      out += ": ";
      i += 1;
    } else if (/\s/.test(ch)) {
      i += 1;
    } else {
      let j = i;
      while (j < text.length && !/[\s,:\]}]/.test(text[j] ?? "")) j += 1;
      out += text.slice(i, j);
      i = j;
    }
  }
  return out;
}

const escapeText = (s: string): string =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const escapeAttr = (s: string): string => escapeText(s).replace(/"/g, "&quot;");

function openTag(el: Element): string {
  const attrs = Array.from(el.attributes)
    .map((a) => ` ${a.name}="${escapeAttr(a.value)}"`)
    .join("");
  return `${el.tagName}${attrs}`;
}

function formatNode(node: Node, depth: number): string[] {
  const pad = INDENT.repeat(depth);
  switch (node.nodeType) {
    case Node.ELEMENT_NODE: {
      const el = node as Element;
      const kids = Array.from(el.childNodes).filter(
        (c) => c.nodeType !== Node.TEXT_NODE || (c.textContent ?? "").trim() !== "",
      );
      if (kids.length === 0) return [`${pad}<${openTag(el)}/>`];
      if (kids.length === 1 && kids[0]?.nodeType === Node.TEXT_NODE) {
        const body = escapeText((kids[0].textContent ?? "").trim());
        return [`${pad}<${openTag(el)}>${body}</${el.tagName}>`];
      }
      return [
        `${pad}<${openTag(el)}>`,
        ...kids.flatMap((kid) => formatNode(kid, depth + 1)),
        `${pad}</${el.tagName}>`,
      ];
    }
    case Node.TEXT_NODE:
      return [`${pad}${escapeText((node.textContent ?? "").trim())}`];
    case Node.CDATA_SECTION_NODE:
      return [`${pad}<![CDATA[${node.textContent ?? ""}]]>`];
    case Node.COMMENT_NODE:
      return [`${pad}<!--${node.textContent ?? ""}-->`];
    case Node.PROCESSING_INSTRUCTION_NODE:
      return [`${pad}<?${node.nodeName} ${node.textContent ?? ""}?>`];
    default:
      return [];
  }
}

export function prettyXml(text: string): string | null {
  const doc = new DOMParser().parseFromString(text, "application/xml");
  if (doc.getElementsByTagName("parsererror").length > 0 || !doc.documentElement) return null;
  const declaration = /^\s*(<\?xml[^>]*\?>)/.exec(text)?.[1];
  const lines = Array.from(doc.childNodes).flatMap((n) => formatNode(n, 0));
  return [...(declaration ? [declaration] : []), ...lines].join("\n");
}
