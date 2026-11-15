import { useEffect, useState } from "react";
import type { RequestLocation } from "../../ipc/bindings";
import { Splitter } from "../../ui/Splitter";
import { Tabs } from "../../ui/Tabs";
import { subscribeCollectionChanged } from "../collections/api";
import "../collections/collections.css";
import { reportError, showToast } from "../../ui/toastStore";
import { importCurl } from "../curl/api";
import { GraphqlEditor, graphqlBodyOf } from "../graphql/GraphqlEditor";
import { runsSubscription } from "../graphql/operations";
import { SubscriptionBar } from "../graphql/SubscriptionBar";
import { SubscriptionPanel } from "../graphql/SubscriptionPanel";
import { subscriptionOf } from "../graphql/subscriptionModel";
import { SubscriptionSettings } from "../graphql/SubscriptionSettings";
import { VariablesEditor } from "../graphql/VariablesEditor";
import SoapPanel from "../soap/SoapPanel";
import { useNetworkStore } from "../network/networkStore";
import { useEffectiveVariables } from "../environments/useEffectiveVariables";
import { ResponseViewer } from "../response/ResponseViewer";
import { AuthEditor } from "./AuthEditor";
import { BodyEditor } from "./BodyEditor";
import { countEnabled, fromRows, toRows } from "./content";
import { KeyValueTable } from "./KeyValueTable";
import { OAuthTokenPanel } from "./OAuthTokenPanel";
import { ParamsEditor } from "./ParamsEditor";
import { sectionsFor } from "./sections";
import { currentContent, useRequestStore } from "./requestStore";
import { ScriptsEditor, type ScriptPhase } from "./ScriptsEditor";
import { appendSnippet } from "./snippets";
import { SettingsEditor } from "./SettingsEditor";
import { UrlBar } from "./UrlBar";
import { ConnectBar } from "../ws/ConnectBar";
import { SubprotocolsEditor } from "../ws/SubprotocolsEditor";
import { TemplatesEditor } from "../ws/TemplatesEditor";
import { WsPanel } from "../ws/WsPanel";
import { wsSettingsOf } from "../ws/wsModel";
import { useWsStore } from "../ws/wsStore";
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

  const kind = content.kind;
  const isWs = kind === "ws";
  const isSubscription = runsSubscription(content);
  const wsSettings = wsSettingsOf(content);
  const sections = sectionsFor(kind, {
    params: tab.paramRows.filter((r) => r.enabled && r.name !== "").length,
    headers: countEnabled(content.headers),
  });
  const shown = sections.some((s) => s.id === section) ? section : "params";

  return (
    <div className="request-editor">
      <div className="request-pane" style={{ height: editorHeight }}>
      {isSubscription ? (
        <SubscriptionBar
          tabId={tabId}
          location={location}
          content={content}
          variables={variables}
          dirty={tab.draft !== null}
          onUrl={(url) => editUrl(tabId, url)}
          onSave={() => void save(tabId, location)}
        />
      ) : isWs ? (
        <ConnectBar
          tabId={tabId}
          location={location}
          content={content}
          variables={variables}
          dirty={tab.draft !== null}
          onUrl={(url) => editUrl(tabId, url)}
          onSave={() => void save(tabId, location)}
        />
      ) : (
        <UrlBar
          method={content.method}
          url={content.url}
          location={location}
          variables={variables}
          sending={tab.sending}
          dirty={tab.draft !== null}
          hideMethod={kind === "graphql"}
          tlsVerified={content.settings.verify_tls}
          proxyActive={proxyActive}
          onImportCurl={(text) => void applyCurl(text)}
          onMethod={(method) => edit(tabId, (c) => ({ ...c, method }))}
          onUrl={(url) => editUrl(tabId, url)}
          onSend={() => void send(tabId, location)}
          onCancel={() => cancel(tabId)}
          onSave={() => void save(tabId, location)}
        />
      )}
      <Tabs tabs={sections} selectedId={shown} onSelect={setSection} label="Request sections">
        {shown === "params" ? <ParamsEditor variables={variables} rows={tab.paramRows} onChange={(rows) => editParams(tabId, rows)} /> : null}
        {shown === "headers" ? (
          <KeyValueTable
            label="Headers"
            variables={variables}
            rows={toRows(content.headers)}
            onChange={(rows) => edit(tabId, (c) => ({ ...c, headers: fromRows(rows) }))}
          />
        ) : null}
        {shown === "body" ? (
          <BodyEditor
            location={location}
            variables={variables}
            body={kind === "soap" && content.body.type !== "xml" ? { type: "xml", text: "" } : content.body}
            allowedTypes={kind === "soap" ? ["xml"] : undefined}
            onChange={(body) => edit(tabId, (c) => ({ ...c, body }))}
          />
        ) : null}
        {shown === "query" ? (
          <GraphqlEditor tabId={tabId} location={location} content={content} variables={variables} onChange={(body) => edit(tabId, (c) => ({ ...c, body }))} />
        ) : null}
        {shown === "variables" ? (
          <VariablesEditor
            value={content.body.type === "graphql" ? content.body.variables : ""}
            variables={variables}
            onChange={(text) => edit(tabId, (c) => ({ ...c, body: { ...graphqlBodyOf(c), variables: text } }))}
          />
        ) : null}
        {shown === "subscription" ? (
          <SubscriptionSettings
            settings={subscriptionOf(content)}
            variables={variables}
            onChange={(subscription) => edit(tabId, (c) => ({ ...c, subscription }))}
          />
        ) : null}
        {shown === "soap" ? <SoapPanel location={location} content={content} variables={variables} onChange={(update) => edit(tabId, update)} /> : null}
        {shown === "messages" ? (
          <TemplatesEditor
            templates={wsSettings.messages}
            onChange={(messages) => edit(tabId, (c) => ({ ...c, ws: { ...wsSettingsOf(c), messages: [...messages] } }))}
            onLoad={(t) => useWsStore.getState().setComposer(tabId, { format: t.format, text: t.text })}
          />
        ) : null}
        {shown === "subprotocols" ? (
          <SubprotocolsEditor
            value={wsSettings.subprotocols}
            onChange={(subprotocols) => edit(tabId, (c) => ({ ...c, ws: { ...wsSettingsOf(c), subprotocols: [...subprotocols] } }))}
          />
        ) : null}
        {shown === "auth" ? (
          <>
            <AuthEditor location={location} variables={variables} auth={content.auth} onChange={(auth) => edit(tabId, (c) => ({ ...c, auth }))} />
            {content.auth.type === "oauth2" ? <OAuthTokenPanel tabId={tabId} location={location} content={content} /> : null}
          </>
        ) : null}
        {shown === "scripts" ? (
          <ScriptsEditor
            scripts={content.scripts}
            phase={phase}
            onPhase={setPhase}
            onChange={(scripts) => edit(tabId, (c) => ({ ...c, scripts }))}
          />
        ) : null}
        {shown === "settings" ? (
          <SettingsEditor connectionOnly={isWs} settings={content.settings} onChange={(settings) => edit(tabId, (c) => ({ ...c, settings }))} />
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
      {isSubscription ? (
        <SubscriptionPanel tabId={tabId} location={location} />
      ) : isWs ? (
        <WsPanel tabId={tabId} location={location} content={content} variables={variables} onChange={(update) => edit(tabId, update)} />
      ) : (
        <ResponseViewer response={tab.response} error={tab.error} sending={tab.sending} scripts={tab.scripts} onInsertScriptLine={insertPostResponseLine} />
      )}
    </div>
  );
}
