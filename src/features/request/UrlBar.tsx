import { useState, type ClipboardEvent } from "react";
import type { EffectiveVariable, RequestLocation } from "../../ipc/bindings";
import { ConfirmDialog } from "../collections/ConfirmDialog";
import { Button } from "../../ui/Button";
import { CodeEditor } from "../../ui/code/CodeEditor";
import { usePreviewTitle } from "../environments/usePreviewTitle";

export interface UrlBarProps {
  readonly method: string;
  readonly url: string;
  readonly location: RequestLocation;
  readonly variables: readonly EffectiveVariable[] | undefined;
  readonly sending: boolean;
  readonly dirty: boolean;
  readonly tlsVerified: boolean;
  /** A custom proxy is configured. */
  readonly proxyActive?: boolean;
  /** Called with a pasted cURL command once the user agrees to replace the request. */
  readonly onImportCurl?: (text: string) => void;
  /** GraphQL requests are always POST: the method select is hidden. */
  readonly hideMethod?: boolean;
  readonly onMethod: (method: string) => void;
  readonly onUrl: (url: string) => void;
  readonly onSend: () => void;
  readonly onCancel: () => void;
  readonly onSave: () => void;
}

export const isCurlText = (text: string): boolean => /^curl\s/i.test(text.trimStart());

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

export function UrlBar(props: UrlBarProps) {
  const { method, url, sending, dirty, tlsVerified } = props;
  const [pastedCurl, setPastedCurl] = useState<string | null>(null);
  const preview = usePreviewTitle(props.location, url);
  const methods = METHODS.includes(method) ? METHODS : [method, ...METHODS];
  const onPaste = (event: ClipboardEvent) => {
    const text = event.clipboardData.getData("text");
    if (!props.onImportCurl || !isCurlText(text)) return;
    event.preventDefault();
    event.stopPropagation();
    setPastedCurl(text);
  };
  return (
    <div className="request-line">
      {props.hideMethod ? null : (
        <select aria-label="Method" value={method} onChange={(e) => props.onMethod(e.target.value)}>
          {methods.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      )}
      <div className="url-field" title={preview} onPasteCapture={onPaste}>
        <CodeEditor
          singleLine
          language="text"
          ariaLabel="URL"
          value={url}
          variables={props.variables}
          onChange={props.onUrl}
        />
      </div>
      {tlsVerified ? null : <span className="tls-badge" title="Certificate checks are off for this request">TLS not verified</span>}
      {props.proxyActive ? <span className="proxy-badge" title="Requests go through a custom proxy">Proxy</span> : null}
      {pastedCurl !== null ? (
        <ConfirmDialog
          title="Import cURL?"
          message="Replace this request with the pasted cURL command?"
          confirmLabel="Replace"
          onConfirm={() => {
            props.onImportCurl?.(pastedCurl);
            setPastedCurl(null);
          }}
          onClose={() => setPastedCurl(null)}
        />
      ) : null}
      {sending ? (
        <Button label="Cancel" onClick={props.onCancel} />
      ) : (
        <Button label="Send" onClick={props.onSend} />
      )}
      <Button label="Save" disabled={!dirty} onClick={props.onSave} />
    </div>
  );
}
