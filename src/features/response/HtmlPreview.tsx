import "./response.css";

/**
 * Untrusted HTML renders only inside a fully sandboxed iframe (no tokens in
 * `sandbox`), with a CSP that forbids every network load.
 */
const PREVIEW_CSP =
  "default-src 'none'; img-src data:; style-src 'unsafe-inline'; form-action 'none'";

export const withPreviewCsp = (html: string): string =>
  `<meta http-equiv="Content-Security-Policy" content="${PREVIEW_CSP}">${html}`;

export interface HtmlPreviewProps {
  readonly html: string;
}

export function HtmlPreview({ html }: HtmlPreviewProps) {
  return <iframe className="html-preview" sandbox="" srcDoc={withPreviewCsp(html)} title="Response preview" />;
}
