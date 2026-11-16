import { useEffect } from "react";
import { useTabsStore } from "../../shell/tabsStore";
import { Button } from "../../ui/Button";
import { Menu } from "../../ui/Menu";
import { reportError } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as api from "./api";
import { newJourney } from "./journeyModel";
import { journeyKey, useJourneyStore } from "./journeyStore";
import "./journeys.css";

const NO_JOURNEYS: readonly never[] = [];

/** Sidebar > Journeys: each open collection's journeys. */
export function JourneysPanel() {
  const trees = useCollectionsStore((s) => s.trees);
  const byCollection = useJourneyStore((s) => s.byCollection);
  const runs = useJourneyStore((s) => s.runs);
  const available = trees.filter((t) => t.collection.available);
  const ids = available.map((t) => t.collection.id).join(",");

  useEffect(() => {
    ids
      .split(",")
      .filter(Boolean)
      .forEach((id) => void useJourneyStore.getState().load(id));
  }, [ids]);

  const create = async (collectionId: string) => {
    try {
      const made = await api.saveJourney(
        collectionId,
        null,
        newJourney("New journey"),
      );
      await useJourneyStore.getState().load(collectionId);
      useTabsStore.getState().openJourney(collectionId, made.file, made.name);
    } catch (err) {
      reportError(err);
    }
  };

  if (available.length === 0)
    return (
      <p className="empty-text">
        No journeys yet. Open a collection to add one.
      </p>
    );

  return (
    <div className="journeys-panel">
      {available.map(({ collection }) => {
        const journeys = byCollection[collection.id] ?? NO_JOURNEYS;
        return (
          <section
            key={collection.id}
            aria-label={`Journeys of ${collection.name}`}
          >
            <div className="panel-actions">
              <strong>{collection.name}</strong>
              <Button
                icon="plus"
                label="New Journey"
                aria-label={`New journey in ${collection.name}`}
                onClick={() => void create(collection.id)}
              />
            </div>
            {journeys.length === 0 ? (
              <p className="empty-text">No journeys yet.</p>
            ) : null}
            <ul
              className="scratch-list"
              aria-label={`Journeys of ${collection.name}`}
            >
              {journeys.map((journey) => {
                const running =
                  runs[journeyKey(collection.id, journey.file)]?.running;
                return (
                  <li key={journey.file} className="tree-row">
                    <span
                      className={
                        running ? "webhook-dot webhook-dot-on" : "webhook-dot"
                      }
                      role="img"
                      aria-label={running ? "Running" : "Idle"}
                    />
                    <button
                      type="button"
                      className="scratch-open"
                      title={journey.error ?? undefined}
                      onClick={() =>
                        useTabsStore
                          .getState()
                          .openJourney(
                            collection.id,
                            journey.file,
                            journey.name,
                          )
                      }
                    >
                      <span className="tree-label">{journey.name}</span>
                      <span className="tree-method">
                        {journey.error ? "!" : journey.nodes}
                      </span>
                    </button>
                    <Menu
                      label={`Actions for ${journey.name}`}
                      icon="chevron-down"
                      className="tree-menu"
                      items={[
                        {
                          label: "Delete",
                          onSelect: () =>
                            void useJourneyStore
                              .getState()
                              .remove(collection.id, journey.file)
                              .then(
                                (done) =>
                                  done &&
                                  useTabsStore
                                    .getState()
                                    .closeJourney(collection.id, journey.file),
                              ),
                        },
                      ]}
                    />
                  </li>
                );
              })}
            </ul>
          </section>
        );
      })}
    </div>
  );
}
