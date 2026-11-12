import { useState } from "react";
import type { AppError, ResponseMeta } from "../../ipc/bindings";
import { toUserMessage } from "../../ipc/errors";
import { Tabs } from "../../ui/Tabs";
import { BodyView } from "./BodyView";
import { formatSize } from "./size";
import "./viewer.css";

export interface ResponseViewerProps {
  readonly response: ResponseMeta | null;
  readonly error: AppError | null;
  readonly sending: boolean;
}

type PaneId = "body" | "headers" | "cookies";

/** `network`, `timeout` and `cancelled` carry user-ready messages from the engine. */
export const errorText = (error: AppError): string =>
  error.code === "cancelled" ? "Cancelled" : error.message.trim() || toUserMessage(error);

const statusClass = (status: number): string => `status-${Math.floor(status / 100)}xx`;

function PairTable({ label, rows }: { readonly label: string; readonly rows: readonly (readonly [string, string])[] }) {
  if (rows.length === 0) return <p className="response-notice">None.</p>;
  return (
    <table className="response-table" aria-label={label}>
      <tbody>
        {rows.map(([name, value], i) => (
          <tr key={`${name}-${i}`}>
            <th scope="row">{name}</th>
            <td>{value}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Loaded({ response }: { readonly response: ResponseMeta }) {
  const [pane, setPane] = useState<PaneId>("body");
  const redirected = response.url !== "";
  return (
    <>
      <div className="response-status" role="status">
        <strong className={statusClass(response.status)}>
          {response.status} {response.status_text}
        </strong>
        <span title={`Time to first byte: ${response.ttfb_ms} ms`}>{response.total_ms} ms</span>
        <span>{formatSize(response.size_bytes)}</span>
        <span>{response.http_version}</span>
        {redirected ? <span title="Final URL">{response.url}</span> : null}
        {response.truncated ? <span className="response-badge">Truncated</span> : null}
        {response.url.startsWith("https:") && !response.tls_verified ? (
          <span className="response-badge">TLS not verified</span>
        ) : null}
      </div>
      {response.unresolved.length > 0 ? (
        <p className="response-notice" role="status">
          Sent with unresolved variables: {response.unresolved.join(", ")}
        </p>
      ) : null}
      <Tabs
        label="Response sections"
        selectedId={pane}
        onSelect={(id) => setPane(id as PaneId)}
        tabs={[
          { id: "body", label: "Body" },
          { id: "headers", label: "Headers", count: response.headers.length },
          { id: "cookies", label: "Cookies", count: response.cookies.length },
        ]}
      >
        {pane === "body" ? <BodyView response={response} /> : null}
        {pane === "headers" ? (
          <PairTable label="Response headers" rows={response.headers.map((h) => [h.name, h.value] as const)} />
        ) : null}
        {pane === "cookies" ? (
          <PairTable
            label="Response cookies"
            rows={response.cookies.map((c) => [c.name, cookieDetail(c)] as const)}
          />
        ) : null}
      </Tabs>
    </>
  );
}

function cookieDetail(c: ResponseMeta["cookies"][number]): string {
  const attrs = [
    c.domain && `Domain=${c.domain}`,
    c.path && `Path=${c.path}`,
    c.expires && `Expires=${c.expires}`,
    c.secure && "Secure",
    c.http_only && "HttpOnly",
  ].filter(Boolean);
  return [c.value, ...attrs].join("; ");
}

export function ResponseViewer({ response, error, sending }: ResponseViewerProps) {
  let content;
  if (sending) content = <p className="response-notice">Sending...</p>;
  else if (error) content = <p className="response-error" role="alert">{errorText(error)}</p>;
  else if (response) content = <Loaded key={response.request_id} response={response} />;
  else content = <p className="response-empty">Send a request to see the response.</p>;
  return <section className="response-viewer" aria-label="Response">{content}</section>;
}
