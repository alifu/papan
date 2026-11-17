import type { EffectiveVariable, GrpcMethod } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { CodeEditor } from "../../ui/code/CodeEditor";
import { jsonWarning } from "../ws/wsModel";
import { differsFromTemplate } from "./grpcModel";
import "./grpc.css";

export interface MessageEditorProps {
  readonly message: string;
  readonly method: GrpcMethod | null;
  readonly variables: readonly EffectiveVariable[] | undefined;
  readonly onChange: (message: string) => void;
}

/** The request message as proto3 JSON, with the method's template one click away. */
export function MessageEditor({ message, method, variables, onChange }: MessageEditorProps) {
  const warning = jsonWarning(message);
  return (
    <div className="grpc-message">
      <div className="ws-composer-bar">
        <Button
          label="Reset to template"
          disabled={method === null || !differsFromTemplate(message, method)}
          title={method === null ? "Pick a method first" : undefined}
          onClick={() => method && onChange(method.template)}
        />
        {method ? <span className="grpc-hint">{method.input_type}</span> : null}
      </div>
      <div className="ws-composer-editor">
        <CodeEditor language="json" ariaLabel="Message" value={message} variables={variables} onChange={onChange} />
      </div>
      {warning ? <p role="status" className="ws-warning">{warning}</p> : null}
    </div>
  );
}
