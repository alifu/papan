import { useEffect, useState } from "react";
import { Button } from "../../ui/Button";
import { useActiveLocation } from "./useActiveLocation";
import { EnvManager } from "./EnvManager";
import { useEnvStore } from "./envStore";
import "./environments.css";

const SCRATCH_TITLE = "Scratch requests use global variables only";
const NO_ENV = "";

/** Toolbar control: pick the environment of the active tab's collection and open the manager. */
export function EnvSwitcher() {
  const location = useActiveLocation();
  const collectionId = location?.scope === "collection" ? location.collection_id : null;
  const list = useEnvStore((s) => (collectionId ? s.byCollection[collectionId] : undefined));
  const storage = useEnvStore((s) => s.storage);
  const [managing, setManaging] = useState(false);

  useEffect(() => {
    void useEnvStore.getState().loadStorage();
  }, []);
  useEffect(() => {
    if (collectionId) void useEnvStore.getState().loadEnvironments(collectionId);
  }, [collectionId]);

  const valid = (list?.environments ?? []).filter((e) => !e.invalid);
  return (
    <div className="env-switcher">
      <select
        aria-label="Environment"
        value={list?.selected ?? NO_ENV}
        disabled={collectionId === null}
        title={collectionId === null ? SCRATCH_TITLE : undefined}
        onChange={(e) => {
          if (collectionId) void useEnvStore.getState().select(collectionId, e.target.value || null);
        }}
      >
        <option value={NO_ENV}>No environment</option>
        {valid.map((env) => (
          <option key={env.env} value={env.env}>
            {env.name}
          </option>
        ))}
      </select>
      <Button label="Variables…" onClick={() => setManaging(true)} />
      {storage && storage.mode !== "keychain" ? (
        <span className="env-badge" role="status" title={storage.message ?? undefined}>
          {storage.mode === "damaged" ? "Secrets unavailable" : "Secrets: session only"}
        </span>
      ) : null}
      {managing ? <EnvManager collectionId={collectionId} onClose={() => setManaging(false)} /> : null}
    </div>
  );
}
