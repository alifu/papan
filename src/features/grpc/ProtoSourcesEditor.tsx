import { useEffect, useState } from "react";
import type { GrpcSettings, GrpcSource, RequestContent, RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { reportError } from "../../ui/toastStore";
import * as api from "./api";
import { grpcSettingsOf, parseImportPaths } from "./grpcModel";
import { useGrpcStore } from "./grpcStore";
import "./grpc.css";

export interface ProtoSourcesEditorProps {
  readonly tabId: string;
  readonly location: RequestLocation;
  readonly content: RequestContent;
  readonly onChange: (update: (content: RequestContent) => RequestContent) => void;
}

const dedupe = (items: readonly string[]): string[] => [...new Set(items)];

/** The Protos section: where service definitions come from, the proto list and compile errors. */
export function ProtoSourcesEditor({ tabId, location, content, onChange }: ProtoSourcesEditorProps) {
  const grpc = grpcSettingsOf(content);
  const tab = useGrpcStore((s) => s.byTab[tabId]);
  const importPathsProp = grpc.import_paths.join(", ");
  const [importPathsText, setImportPathsText] = useState(importPathsProp);
  // Typing keeps the raw text (a trailing comma); an outside change (reload, revert) replaces it.
  useEffect(() => {
    setImportPathsText((text) => (parseImportPaths(text).join(", ") === importPathsProp ? text : importPathsProp));
  }, [importPathsProp]);
  const edit = (patch: Partial<GrpcSettings>) => onChange((c) => ({ ...c, grpc: { ...grpcSettingsOf(c), ...patch } }));

  const importProto = async () => {
    try {
      const imported = await api.grpcImportProto(location);
      if (imported.added.length === 0) return;
      onChange((c) => {
        const current = grpcSettingsOf(c);
        return { ...c, grpc: { ...current, protos: dedupe([...current.protos, ...imported.added]) } };
      });
    } catch (err) {
      reportError(err);
    }
  };
  const warnings = tab?.schema?.warnings ?? [];

  return (
    <div className="editor-form">
      <label>
        Source
        <select aria-label="Source" value={grpc.source} onChange={(e) => edit({ source: e.target.value as GrpcSource })}>
          <option value="protos">Proto files</option>
          <option value="reflection">Server reflection</option>
        </select>
      </label>
      {grpc.source === "protos" ? (
        <>
          <ul className="grpc-protos" aria-label="Proto files">
            {grpc.protos.map((proto) => (
              <li key={proto} className="ws-template-row">
                <span className="ws-preview">{proto}</span>
                <Button
                  label="Remove"
                  aria-label={`Remove ${proto}`}
                  onClick={() => edit({ protos: grpc.protos.filter((p) => p !== proto) })}
                />
              </li>
            ))}
          </ul>
          <div className="ws-composer-bar">
            <Button label="Import .proto…" onClick={() => void importProto()} />
          </div>
          <label>
            Import paths (comma separated, relative to the protos folder)
            <input
              aria-label="Import paths"
              value={importPathsText}
              onChange={(e) => {
                setImportPathsText(e.target.value);
                edit({ import_paths: parseImportPaths(e.target.value) });
              }}
            />
          </label>
        </>
      ) : (
        <div className="ws-composer-bar">
          <Button
            label={tab?.schemaLoading ? "Fetching…" : tab?.schema?.fetched_at_ms ? "Refresh" : "Load schema from server"}
            disabled={tab?.schemaLoading === true}
            onClick={() => void useGrpcStore.getState().reflect(tabId, location, content)}
          />
          <span className="grpc-hint">
            {tab?.schema?.fetched_at_ms ? `Fetched ${new Date(tab.schema.fetched_at_ms).toLocaleString()}` : "Not fetched yet"}
          </span>
          <p role="note" className="grpc-hint">
            {tab?.schemaNeedsRefresh ? "No cached schema for this server. " : ""}
            Loading from the server sends this request's metadata, including the Authorization header, to {content.url || "the URL"}.
          </p>
        </div>
      )}
      {tab?.schemaError ? (
        <p role="alert" className="field-error grpc-compile-error">
          {tab.schemaError.message}
        </p>
      ) : null}
      {warnings.map((w) => (
        <p key={w} role="status" className="ws-warning">
          {w}
        </p>
      ))}
    </div>
  );
}
