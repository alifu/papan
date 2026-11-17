import { useEffect, useState } from "react";
import type {
  EffectiveVariable,
  RequestContent,
  RequestLocation,
  SoapTemplate,
  SoapVersion,
  WsdlImport,
  WsdlRef,
} from "../../ipc/bindings";
import { reportError } from "../../ui/toastStore";
import { Button } from "../../ui/Button";
import { CodeEditor } from "../../ui/code/CodeEditor";
import { ConfirmDialog } from "../collections/ConfirmDialog";
import * as api from "./api";
import { applyTemplate, needsConfirmation } from "./applyTemplate";
import { OperationList, type OperationSelection } from "./OperationList";
import { WsdlImportDialog } from "./WsdlImportDialog";
import "./soap.css";

export interface SoapPanelProps {
  readonly location: RequestLocation;
  readonly content: RequestContent;
  readonly variables?: readonly EffectiveVariable[];
  /** Receives a pure update; the owner applies it to its current content. */
  readonly onChange: (update: (content: RequestContent) => RequestContent) => void;
}

interface PendingApply {
  readonly template: SoapTemplate;
  readonly ref: WsdlRef;
}

const baseName = (path: string): string => path.split(/[\\/]/).pop() ?? path;

/** The backend says what to save: `ref_url` for a URL import, empty for a picked file. */
const matchesRef = (wsdl: WsdlImport, ref: WsdlRef): boolean =>
  wsdl.ref_url !== "" ? wsdl.ref_url === ref.url : baseName(wsdl.source) === ref.file;

const refFor = (wsdl: WsdlImport, s: OperationSelection): WsdlRef => ({
  url: wsdl.ref_url,
  file: wsdl.ref_url !== "" ? "" : baseName(wsdl.source),
  ...s,
});

/** SOAP tab: version, SOAPAction, WSDL import and envelope generation into the XML body. */
export default function SoapPanel({ location, content, variables, onChange }: SoapPanelProps) {
  const { soap } = content;
  const [wsdl, setWsdl] = useState<WsdlImport | null>(null);
  const [importing, setImporting] = useState(false);
  const [includeOptional, setIncludeOptional] = useState(true);
  const [pending, setPending] = useState<PendingApply | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const saved = soap.wsdl;
  const hasSaved = saved !== null && (saved.url !== "" || saved.file !== "");

  useEffect(() => {
    if (!saved || !hasSaved || (wsdl && matchesRef(wsdl, saved))) return;
    let live = true;
    api
      .readCachedWsdl(location, saved)
      .then((cached) => {
        if (!live) return;
        setWsdl(cached);
        setNotice(cached ? null : "The WSDL is not cached on this machine. Import it again to pick operations.");
      })
      .catch(reportError);
    return () => {
      live = false;
    };
    // Reload only when the referenced document changes, not on every operation pick.
  }, [location, saved?.url, saved?.file]);

  const selected: OperationSelection | null =
    saved && saved.operation !== ""
      ? { service: saved.service, port: saved.port, operation: saved.operation }
      : null;

  const commit = (template: SoapTemplate, ref: WsdlRef, replaceUrl: boolean) =>
    onChange((current) => applyTemplate(current, template, ref, replaceUrl));

  const generate = async (target: WsdlImport, choice: OperationSelection) => {
    try {
      const template = await api.soapEnvelope(target.id, choice.service, choice.port, choice.operation, {
        include_optional: includeOptional,
      });
      const ref = refFor(target, choice);
      if (needsConfirmation(content, template)) setPending({ template, ref });
      else commit(template, ref, false);
    } catch (err) {
      reportError(err);
    }
  };

  const setVersion = (version: SoapVersion) => onChange((c) => ({ ...c, soap: { ...c.soap, version } }));
  const setAction = (action: string) => onChange((c) => ({ ...c, soap: { ...c.soap, action } }));

  return (
    <div className="soap-panel">
      <div className="soap-row">
        <label>
          <span>SOAP version</span>
          <select value={soap.version} onChange={(e) => setVersion(e.target.value as SoapVersion)}>
            <option value="1.1">SOAP 1.1</option>
            <option value="1.2">SOAP 1.2</option>
          </select>
        </label>
        <div className="soap-action">
          <span>SOAPAction</span>
          <CodeEditor
            singleLine
            language="text"
            ariaLabel="SOAPAction"
            value={soap.action}
            variables={variables}
            onChange={setAction}
          />
        </div>
      </div>
      <div className="soap-row">
        <Button label="Import WSDL…" onClick={() => setImporting(true)} />
        <label className="soap-switch">
          <input
            type="checkbox"
            checked={includeOptional}
            onChange={(e) => setIncludeOptional(e.target.checked)}
          />
          <span>Include optional elements</span>
        </label>
        <Button
          label="Regenerate envelope"
          disabled={!wsdl || !selected}
          onClick={() => wsdl && selected && void generate(wsdl, selected)}
        />
      </div>
      {hasSaved ? (
        <p className="soap-note">WSDL: {saved.url || saved.file}</p>
      ) : (
        <p className="soap-note">No WSDL imported. You can still write the envelope by hand.</p>
      )}
      {notice ? <p className="soap-note">{notice}</p> : null}
      {wsdl ? (
        <OperationList wsdl={wsdl} selected={selected} onSelect={(choice) => void generate(wsdl, choice)} />
      ) : null}
      {importing ? (
        <WsdlImportDialog
          location={location}
          onImported={(imported) => {
            setWsdl(imported);
            setNotice(null);
          }}
          onClose={() => setImporting(false)}
        />
      ) : null}
      {pending ? (
        <ConfirmDialog
          title="Replace envelope?"
          message="This replaces the current body and, if it differs, the URL with the generated ones."
          confirmLabel="Replace"
          onConfirm={() => {
            commit(pending.template, pending.ref, true);
            setPending(null);
          }}
          onClose={() => setPending(null)}
        />
      ) : null}
    </div>
  );
}
