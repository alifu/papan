import type { EffectiveVariable, RequestLocation } from "../../ipc/bindings";
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
  readonly onMethod: (method: string) => void;
  readonly onUrl: (url: string) => void;
  readonly onSend: () => void;
  readonly onCancel: () => void;
  readonly onSave: () => void;
}

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

export function UrlBar(props: UrlBarProps) {
  const { method, url, sending, dirty, tlsVerified } = props;
  const preview = usePreviewTitle(props.location, url);
  const methods = METHODS.includes(method) ? METHODS : [method, ...METHODS];
  return (
    <div className="request-line">
      <select aria-label="Method" value={method} onChange={(e) => props.onMethod(e.target.value)}>
        {methods.map((m) => (
          <option key={m} value={m}>
            {m}
          </option>
        ))}
      </select>
      <div className="url-field" title={preview}>
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
      {sending ? (
        <Button label="Cancel" onClick={props.onCancel} />
      ) : (
        <Button label="Send" onClick={props.onSend} />
      )}
      <Button label="Save" disabled={!dirty} onClick={props.onSave} />
    </div>
  );
}
