import { useEffect, useState } from "react";
import { useTabsStore } from "../../shell/tabsStore";
import { Button } from "../../ui/Button";
import { Tabs } from "../../ui/Tabs";
import { reportError, showToast } from "../../ui/toastStore";
import { copyText } from "../response/clipboard";
import * as api from "./api";
import { MockLog } from "./MockLog";
import { MockSettings } from "./MockSettings";
import {
  draftProblem,
  endpointLabel,
  newRoute,
  removeAt,
  replaceAt,
  routeFromRequest,
  normalizeMock,
  type MEnv,
} from "./mockModel";
import { mockKey, useMockStore } from "./mockStore";
import { RouteEditor } from "./RouteEditor";
import "./mocks.css";

const NO_ENTRIES: readonly never[] = [];
const NO_MOCKS: readonly never[] = [];

export interface MockTabProps {
  readonly collectionId: string;
  readonly file: string;
  readonly tabId: string;
}

export function MockTab({ collectionId, file, tabId }: MockTabProps) {
  const [draft, setDraft] = useState<MEnv | null>(null);
  const [saved, setSaved] = useState<MEnv | null>(null);
  const [selected, setSelected] = useState(0);
  const [view, setView] = useState("design");
  const summary = useMockStore((s) =>
    (s.byCollection[collectionId] ?? NO_MOCKS).find((m) => m.file === file),
  );
  const entries = useMockStore(
    (s) => s.logs[mockKey(collectionId, file)] ?? NO_ENTRIES,
  );
  const store = useMockStore.getState();

  useEffect(() => {
    let current = true;
    api
      .readMock(collectionId, file)
      .then((mock) => {
        if (!current) return;
        const normal = normalizeMock(mock);
        setDraft(normal);
        setSaved(normal);
      })
      .catch(reportError);
    void useMockStore.getState().load(collectionId);
    void useMockStore.getState().loadLog(collectionId, file);
    return () => {
      current = false;
    };
  }, [collectionId, file]);

  if (!draft) return <p className="empty-text">Loading…</p>;
  const running = summary?.running ?? false;
  const url = summary?.url ?? null;
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);
  const problem = draftProblem(draft);
  const route = draft.routes[selected];

  const save = async (): Promise<boolean> => {
    if (problem) {
      showToast({ kind: "error", message: problem });
      return false;
    }
    try {
      await api.saveMock(collectionId, file, draft);
      setSaved(draft);
      useTabsStore.getState().retitleMock(tabId, draft.name);
      await store.load(collectionId);
      return true;
    } catch (err) {
      reportError(err);
      return false;
    }
  };

  const toggle = async () => {
    if (running) return store.stop(collectionId, file);
    if (dirty && !(await save())) return;
    await store.start(collectionId, file);
  };

  const copyUrl = async () => {
    if (!url) return;
    try {
      await copyText(url);
      showToast({ kind: "info", message: "URL copied." });
    } catch (err) {
      reportError(err);
    }
  };

  const setRoute = (next: typeof route) =>
    next &&
    setDraft({
      ...draft,
      routes: [...replaceAt(draft.routes, selected, next)],
    });

  const addRoute = () => {
    setDraft({ ...draft, routes: [...draft.routes, newRoute()] });
    setSelected(draft.routes.length);
  };

  const removeRoute = () => {
    setDraft({ ...draft, routes: [...removeAt(draft.routes, selected)] });
    setSelected(Math.max(0, selected - 1));
  };

  return (
    <div className="mock-tab">
      <div className="mock-head">
        <h1>{draft.name}</h1>
        <span>{running ? "Running" : "Stopped"}</span>
        <Button
          icon={running ? "stop" : "play"}
          label={running ? "Stop" : "Start"}
          onClick={() => void toggle()}
        />
        <Button label="Save" disabled={!dirty} onClick={() => void save()} />
        {url ? (
          <>
            <code className="mock-url">
              {url}
              {draft.prefix ? `/${draft.prefix.replace(/^\/+|\/+$/g, "")}` : ""}
            </code>
            <Button label="Copy URL" onClick={() => void copyUrl()} />
          </>
        ) : null}
        {running && dirty ? (
          <small>Unsaved changes: saving restarts the mock.</small>
        ) : null}
      </div>
      <Tabs
        label="Mock sections"
        tabs={[
          { id: "design", label: "Routes" },
          { id: "log", label: "Log", count: entries.length },
        ]}
        selectedId={view}
        onSelect={setView}
      >
        {view === "log" ? (
          <MockLog
            entries={entries}
            onClear={() => void store.clearLog(collectionId, file)}
            onCreateRoute={(entry) => {
              setDraft({
                ...draft,
                routes: [
                  ...draft.routes,
                  routeFromRequest(entry, draft.prefix),
                ],
              });
              setSelected(draft.routes.length);
              setView("design");
            }}
          />
        ) : (
          <>
            <MockSettings mock={draft} onChange={setDraft} />
            <div className="mock-routes">
              <ul aria-label="Routes" className="mock-route-list">
                {draft.routes.map((r, index) => (
                  <li key={r.id}>
                    <button
                      type="button"
                      className={
                        index === selected
                          ? "mock-route-item mock-route-active"
                          : "mock-route-item"
                      }
                      aria-current={index === selected}
                      onClick={() => setSelected(index)}
                    >
                      {endpointLabel(r)}
                      {r.enabled ? "" : " (off)"}
                    </button>
                  </li>
                ))}
                <li>
                  <Button icon="plus" label="Add route" onClick={addRoute} />
                </li>
              </ul>
              {route ? (
                <div className="mock-route-pane">
                  <RouteEditor route={route} onChange={setRoute} />
                  <Button label="Delete route" onClick={removeRoute} />
                </div>
              ) : (
                <p className="empty-text">Add a route to start.</p>
              )}
            </div>
          </>
        )}
      </Tabs>
    </div>
  );
}
