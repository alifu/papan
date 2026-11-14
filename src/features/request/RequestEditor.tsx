import { useEffect, useState } from "react";
import type { RequestLocation } from "../../ipc/bindings";
import { Splitter } from "../../ui/Splitter";
import { Tabs } from "../../ui/Tabs";
import { subscribeCollectionChanged } from "../collections/api";
import "../collections/collections.css";
import { reportError, showToast } from "../../ui/toastStore";
import { importCurl } from "../curl/api";
import { useNetworkStore } from "../network/networkStore";
import { useEffectiveVariables } from "../environments/useEffectiveVariables";
import { ResponseViewer } from "../response/ResponseViewer";
import { AuthEditor } from "./AuthEditor";
import { BodyEditor } from "./BodyEditor";
import { countEnabled, fromRows, toRows } from "./content";
import { KeyValueTable } from "./KeyValueTable";
import { OAuthTokenPanel } from "./OAuthTokenPanel";
import { ParamsEditor } from "./ParamsEditor";
import { currentContent, useRequestStore } from "./requestStore";
import { ScriptsEditor, type ScriptPhase } from "./ScriptsEditor";
import { appendSnippet } from "./snippets";
import { SettingsEditor } from "./SettingsEditor";
import { UrlBar } from "./UrlBar";
import "./request.css";

export interface RequestEditorProps {
  readonly tabId: string;
  readonly location: RequestLocation;
}

const EDITOR_MIN_HEIGHT = 120;
const EDITOR_MAX_HEIGHT = 600;
const EDITOR_DEFAULT_HEIGHT = 260;
const EDITOR_KEY_STEP = 16;
const NOT_FOUND_TEXT = "This request no longer exists on disk.";
const locationKey = (loc: RequestLocation): string =>
  loc.scope === "scratch" ? `scratch:${loc.id}` : `collection:${loc.collection_id}:${loc.path}`;

export function RequestEditor({ tabId, location }: RequestEditorProps) {
  const key = locationKey(location);
  const tab = useRequestStore((s) => s.byTab[tabId]);
  const { load, edit, editUrl, editParams, save, send, cancel } = useRequestStore.getState();
  const variables = useEffectiveVariables(location);
  const proxyActive = useNetworkStore((s) => s.proxyMode === "custom");

  const applyCurl = async (text: string) => {
    try {
      const imported = await importCurl(text);
      edit(tabId, (c) => ({ ...c, ...imported.content }));
      editUrl(tabId, imported.content.url);
      if (imported.warnings.length > 0) showToast({ kind: "info", message: imported.warnings.join(" ") });
    } catch (err) {
      reportError(err);
    }
  };
  const [section, setSection] = useState("params");
  const [phase, setPhase] = useState<ScriptPhase>("pre_request");
  const [editorHeight, setEditorHeight] = useState(EDITOR_DEFAULT_HEIGHT);
  const insertPostResponseLine = (line: string) => {
    edit(tabId, (c) => ({ ...c, scripts: { ...c.scripts, post_response: appendSnippet(c.scripts.post_response, line) } }));
    setPhase("post_response");
    setSection("scripts");
  };
  const resize = (h: number) => setEditorHeight(Math.min(EDITOR_MAX_HEIGHT, Math.max(EDITOR_MIN_HEIGHT, h)));

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void load(tabId, location);
    if (location.scope === "collection") {
      const collectionId = location.collection_id;
      void subscribeCollectionChanged((changed) => {
        if (changed === collectionId) void load(tabId, location);
      }).then((fn) => {
        if (active) unlisten = fn;
        else fn();
      });
    }
    return () => {
      active = false;
      unlisten?.();
    };
    // `key` identifies `location`; depending on the object would resubscribe on every render.
  }, [tabId, key]);

  if (tab?.missing) return <p className="empty-text">{NOT_FOUND_TEXT}</p>;
  const content = currentContent(tab);
  if (!tab || !content) return null;

  const sections = [
    { id: "params", label: "Params", count: tab.paramRows.filter((r) => r.enabled && r.name !== "").length },
    { id: "headers", label: "Headers", count: countEnabled(content.headers) },
    { id: "body", label: "Body" },
    { id: "auth", label: "Auth" },
    { id: "scripts", label: "Scripts" },
    { id: "settings", label: "Settings" },
  ];

  return (
    <div className="request-editor">
      <div className="request-pane" style={{ height: editorHeight }}>
      <UrlBar
        method={content.method}
        url={content.url}
        location={location}
        variables={variables}
        sending={tab.sending}
        dirty={tab.draft !== null}
        tlsVerified={content.settings.verify_tls}
        proxyActive={proxyActive}
        onImportCurl={(text) => void applyCurl(text)}
        onMethod={(method) => edit(tabId, (c) => ({ ...c, method }))}
        onUrl={(url) => editUrl(tabId, url)}
        onSend={() => void send(tabId, location)}
        onCancel={() => cancel(tabId)}
        onSave={() => void save(tabId, location)}
      />
      <Tabs tabs={sections} selectedId={section} onSelect={setSection} label="Request sections">
        {section === "params" ? <ParamsEditor variables={variables} rows={tab.paramRows} onChange={(rows) => editParams(tabId, rows)} /> : null}
        {section === "headers" ? (
          <KeyValueTable
            label="Headers"
            variables={variables}
            rows={toRows(content.headers)}
            onChange={(rows) => edit(tabId, (c) => ({ ...c, headers: fromRows(rows) }))}
          />
        ) : null}
        {section === "body" ? (
          <BodyEditor location={location} variables={variables} body={content.body} onChange={(body) => edit(tabId, (c) => ({ ...c, body }))} />
        ) : null}
        {section === "auth" ? (
          <>
            <AuthEditor location={location} variables={variables} auth={content.auth} onChange={(auth) => edit(tabId, (c) => ({ ...c, auth }))} />
            {content.auth.type === "oauth2" ? <OAuthTokenPanel tabId={tabId} location={location} content={content} /> : null}
          </>
        ) : null}
        {section === "scripts" ? (
          <ScriptsEditor
            scripts={content.scripts}
            phase={phase}
            onPhase={setPhase}
            onChange={(scripts) => edit(tabId, (c) => ({ ...c, scripts }))}
          />
        ) : null}
        {section === "settings" ? (
          <SettingsEditor settings={content.settings} onChange={(settings) => edit(tabId, (c) => ({ ...c, settings }))} />
        ) : null}
      </Tabs>
      </div>
      <Splitter
        orientation="horizontal"
        label="Resize response"
        value={editorHeight}
        min={EDITOR_MIN_HEIGHT}
        max={EDITOR_MAX_HEIGHT}
        step={EDITOR_KEY_STEP}
        onResize={resize}
        onToggle={() => resize(EDITOR_DEFAULT_HEIGHT)}
      />
      <ResponseViewer response={tab.response} error={tab.error} sending={tab.sending} scripts={tab.scripts} onInsertScriptLine={insertPostResponseLine} />
    </div>
  );
}
