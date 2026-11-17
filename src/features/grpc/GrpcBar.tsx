import type { EffectiveVariable, GrpcMethod, RequestContent, RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { CodeEditor } from "../../ui/code/CodeEditor";
import { usePreviewTitle } from "../environments/usePreviewTitle";
import {
  allowedActions,
  findMethod,
  grpcSettingsOf,
  KIND_TAG,
  messageAfterPick,
  methodLabel,
  STATUS_LABEL,
  transportOf,
} from "./grpcModel";
import { useGrpcStore } from "./grpcStore";
import { useGrpcSchema } from "./useGrpcSchema";
import "./grpc.css";

export interface GrpcBarProps {
  readonly tabId: string;
  readonly location: RequestLocation;
  readonly content: RequestContent;
  readonly variables: readonly EffectiveVariable[] | undefined;
  readonly dirty: boolean;
  readonly onUrl: (url: string) => void;
  /** Receives a pure update; the owner applies it to its current content. */
  readonly onChange: (update: (content: RequestContent) => RequestContent) => void;
  readonly onSave: () => void;
}

const TRANSPORT_BADGE = { tls: "TLS", plaintext: "Plaintext", unknown: "" } as const;
const pickValue = (service: string, method: string): string => `${service}/${method}`;

/** The gRPC bar: URL, service/method picker, Start/Cancel, Send/End for client streams and the call status. */
export function GrpcBar({ tabId, location, content, variables, dirty, onUrl, onChange, onSave }: GrpcBarProps) {
  useGrpcSchema(tabId, location, content);
  const tab = useGrpcStore((s) => s.byTab[tabId]);
  const grpc = grpcSettingsOf(content);
  const schema = tab?.schema ?? null;
  const selected = findMethod(schema, grpc.service, grpc.method);
  const status = tab?.status ?? "idle";
  const actions = allowedActions(selected?.kind ?? null, status);
  const preview = usePreviewTitle(location, content.url);
  const store = useGrpcStore.getState;
  const transport = transportOf(content.url);
  const canStart = actions.start && grpc.service !== "" && grpc.method !== "";

  const start = () => void store().start(tabId, location, content);
  const send = () => void store().send(tabId, location, grpc.message);
  const pick = (value: string) => {
    const next = schema?.services.flatMap((s) => s.methods.map((m) => ({ service: s.full_name, method: m }))).find(
      (c) => pickValue(c.service, c.method.name) === value,
    );
    if (!next) return;
    onChange((c) => {
      const current = grpcSettingsOf(c);
      const message = messageAfterPick(current.message, findMethod(schema, current.service, current.method), next.method);
      return { ...c, grpc: { ...current, service: next.service, method: next.method.name, message } };
    });
  };

  return (
    <div
      className="request-line"
      onKeyDown={(e) => {
        if (!(e.metaKey || e.ctrlKey) || e.key !== "Enter") return;
        e.preventDefault();
        if (canStart) start();
        else if (actions.send) send();
      }}
    >
      <div className="url-field" title={preview}>
        <CodeEditor singleLine language="text" ariaLabel="URL" value={content.url} variables={variables} onChange={onUrl} />
      </div>
      {TRANSPORT_BADGE[transport] === "" ? null : (
        <span className={`tls-badge grpc-transport-${transport}`} title={transport === "tls" ? "Encrypted (HTTP/2 over TLS)" : "Not encrypted (HTTP/2 cleartext)"}>
          {TRANSPORT_BADGE[transport]}
        </span>
      )}
      <MethodPicker
        schema={schema}
        service={grpc.service}
        method={grpc.method}
        found={selected}
        onPick={pick}
      />
      <span className={`ws-status grpc-status-${status}`} aria-label="Call status">
        {STATUS_LABEL[status]}
      </span>
      {actions.cancel ? (
        <Button label="Cancel" onClick={() => void store().cancel(tabId)} />
      ) : (
        <Button label="Start" disabled={!canStart} onClick={start} title={canStart ? undefined : "Pick a method first"} />
      )}
      {selected?.kind === "client_streaming" || selected?.kind === "bidi" ? (
        <>
          <Button label="Send" disabled={!actions.send} onClick={send} title={actions.send ? undefined : "Start the call first"} />
          <Button label="End" disabled={!actions.end} onClick={() => void store().end(tabId)} />
        </>
      ) : null}
      <Button label="Save" disabled={!dirty} onClick={onSave} />
    </div>
  );
}

interface MethodPickerProps {
  readonly schema: { readonly services: readonly { readonly full_name: string; readonly methods: readonly GrpcMethod[] }[] } | null;
  readonly service: string;
  readonly method: string;
  readonly found: GrpcMethod | null;
  readonly onPick: (value: string) => void;
}

function MethodPicker({ schema, service, method, found, onPick }: MethodPickerProps) {
  const value = pickValue(service, method);
  const missing = service !== "" && found === null;
  return (
    <select aria-label="Method" className="grpc-method" value={service === "" ? "" : value} onChange={(e) => onPick(e.target.value)}>
      {service === "" ? <option value="">Pick a method</option> : null}
      {missing ? <option value={value}>{`${methodLabel(service, method)} (not found)`}</option> : null}
      {(schema?.services ?? []).map((s) => (
        <optgroup key={s.full_name} label={s.full_name}>
          {s.methods.map((m) => (
            <option key={m.name} value={pickValue(s.full_name, m.name)}>
              {`${m.name} (${KIND_TAG[m.kind]})`}
            </option>
          ))}
        </optgroup>
      ))}
    </select>
  );
}
