import { useEffect } from "react";
import type { MockImportFormat } from "../../ipc/bindings";
import { useTabsStore } from "../../shell/tabsStore";
import { Button } from "../../ui/Button";
import { Menu } from "../../ui/Menu";
import { reportError, showToast } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as api from "./api";
import { newMock } from "./mockModel";
import { useMockStore } from "./mockStore";
import "./mocks.css";

const NO_MOCKS: readonly never[] = [];

/** Sidebar > Mocks: each open collection's mock environments, with start/stop. */
export function MocksPanel() {
  const trees = useCollectionsStore((s) => s.trees);
  const byCollection = useMockStore((s) => s.byCollection);
  const ids = trees
    .filter((t) => t.collection.available)
    .map((t) => t.collection.id)
    .join(",");

  useEffect(() => {
    ids
      .split(",")
      .filter(Boolean)
      .forEach((id) => void useMockStore.getState().load(id));
  }, [ids]);

  const create = async (collectionId: string) => {
    try {
      const made = await api.saveMock(collectionId, null, newMock("New mock"));
      await useMockStore.getState().load(collectionId);
      useTabsStore.getState().openMock(collectionId, made.file, made.name);
    } catch (err) {
      reportError(err);
    }
  };

  const fromRequests = async (collectionId: string) => {
    try {
      const made = await api.mockFromCollection(collectionId);
      await useMockStore.getState().load(collectionId);
      useTabsStore.getState().openMock(collectionId, made.file, made.name);
    } catch (err) {
      reportError(err);
    }
  };

  const importFile = async (collectionId: string, format: MockImportFormat) => {
    try {
      const done = await api.importMock(collectionId, format);
      if (!done) return; // the file dialog was cancelled
      await useMockStore.getState().load(collectionId);
      useTabsStore
        .getState()
        .openMock(collectionId, done.summary.file, done.summary.name);
      if (done.notes.length > 0)
        showToast({
          kind: "info",
          message: `Imported with changes: ${done.notes.join(" ")}`,
        });
    } catch (err) {
      reportError(err);
    }
  };

  const exportFile = async (collectionId: string, file: string) => {
    try {
      const saved = await api.exportMock(collectionId, file);
      if (saved) showToast({ kind: "info", message: `Saved ${saved}.` });
    } catch (err) {
      reportError(err);
    }
  };

  const available = trees.filter((t) => t.collection.available);
  if (available.length === 0)
    return (
      <p className="empty-text">
        No mock servers yet. Open a collection to add one.
      </p>
    );

  return (
    <div className="mocks-panel">
      {available.map(({ collection }) => {
        const mocks = byCollection[collection.id] ?? NO_MOCKS;
        return (
          <section
            key={collection.id}
            aria-label={`Mocks of ${collection.name}`}
          >
            <div className="panel-actions">
              <strong>{collection.name}</strong>
              <Button
                icon="plus"
                label="New Mock"
                aria-label={`New mock in ${collection.name}`}
                onClick={() => void create(collection.id)}
              />
              <Button
                label="From Requests"
                aria-label={`Mock from the requests of ${collection.name}`}
                onClick={() => void fromRequests(collection.id)}
              />
              <Menu
                label={`Import a mock into ${collection.name}`}
                icon="chevron-down"
                items={[
                  {
                    label: "Mockoon file…",
                    onSelect: () => void importFile(collection.id, "mockoon"),
                  },
                  {
                    label: "OpenAPI file…",
                    onSelect: () => void importFile(collection.id, "openapi"),
                  },
                ]}
              />
            </div>
            {mocks.length === 0 ? (
              <p className="empty-text">No mock servers yet.</p>
            ) : null}
            <ul
              className="scratch-list"
              aria-label={`Mock servers of ${collection.name}`}
            >
              {mocks.map((mock) => (
                <li key={mock.file} className="tree-row">
                  <span
                    className={
                      mock.running
                        ? "webhook-dot webhook-dot-on"
                        : "webhook-dot"
                    }
                    role="img"
                    aria-label={mock.running ? "Running" : "Stopped"}
                  />
                  <button
                    type="button"
                    className="scratch-open"
                    title={mock.error ?? undefined}
                    onClick={() =>
                      useTabsStore
                        .getState()
                        .openMock(collection.id, mock.file, mock.name)
                    }
                  >
                    <span className="tree-label">{mock.name}</span>
                    <span className="tree-method">
                      {mock.error ? "!" : mock.routes}
                    </span>
                  </button>
                  {mock.error ? null : (
                    <Button
                      icon={mock.running ? "stop" : "play"}
                      className="tree-menu"
                      aria-label={`${mock.running ? "Stop" : "Start"} ${mock.name}`}
                      onClick={() => {
                        const store = useMockStore.getState();
                        void (mock.running
                          ? store.stop(collection.id, mock.file)
                          : store.start(collection.id, mock.file));
                      }}
                    />
                  )}
                  <Menu
                    label={`Actions for ${mock.name}`}
                    icon="chevron-down"
                    className="tree-menu"
                    items={[
                      {
                        label: "Export as Mockoon…",
                        disabled: Boolean(mock.error),
                        onSelect: () =>
                          void exportFile(collection.id, mock.file),
                      },
                      {
                        label: "Delete",
                        onSelect: () =>
                          void useMockStore
                            .getState()
                            .remove(collection.id, mock.file)
                            .then(
                              (done) =>
                                done &&
                                useTabsStore
                                  .getState()
                                  .closeMock(collection.id, mock.file),
                            ),
                      },
                    ]}
                  />
                </li>
              ))}
            </ul>
          </section>
        );
      })}
    </div>
  );
}
