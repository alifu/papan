import { useEffect, useState } from "react";
import type { RequestDoc, RequestLocation } from "../../ipc/bindings";
import { isAppError } from "../../ipc/errors";
import { Button } from "../../ui/Button";
import { reportError } from "../../ui/toastStore";
import { readRequest, subscribeCollectionChanged, writeRequest } from "../collections/api";
import "../collections/collections.css";

export interface RequestPlaceholderProps {
  readonly location: RequestLocation;
}

interface Draft {
  readonly method: string;
  readonly url: string;
}

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
const NOT_FOUND_TEXT = "This request no longer exists on disk.";

const locationKey = (loc: RequestLocation): string =>
  loc.scope === "scratch" ? `scratch:${loc.id}` : `collection:${loc.collection_id}:${loc.path}`;

/** Minimal request editor: method, URL and Save. Edits survive external reloads. */
export function RequestPlaceholder({ location }: RequestPlaceholderProps) {
  const key = locationKey(location);
  const [doc, setDoc] = useState<RequestDoc | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [missing, setMissing] = useState(false);

  useEffect(() => {
    let active = true;
    const load = async () => {
      try {
        const loaded = await readRequest(location);
        if (!active) return;
        setDoc(loaded);
        setMissing(false);
      } catch (err) {
        if (!active) return;
        if (isAppError(err) && err.code === "not_found") setMissing(true);
        else reportError(err);
      }
    };
    void load();

    let unlisten: (() => void) | undefined;
    if (location.scope === "collection") {
      const collectionId = location.collection_id;
      void subscribeCollectionChanged((changedId) => {
        if (changedId === collectionId) void load();
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
  }, [key]);

  if (missing) return <p className="empty-text">{NOT_FOUND_TEXT}</p>;
  if (!doc) return null;

  const shown: Draft = draft ?? { method: doc.method, url: doc.url };
  const methods = METHODS.includes(doc.method) ? METHODS : [doc.method, ...METHODS];
  const edit = (patch: Partial<Draft>) => {
    const next = { ...shown, ...patch };
    setDraft(next.method === doc.method && next.url === doc.url ? null : next);
  };

  const save = async () => {
    try {
      await writeRequest(location, shown);
      setDoc({ ...doc, ...shown });
      setDraft(null);
    } catch (err) {
      reportError(err);
    }
  };

  return (
    <div className="request-placeholder">
      <h1>{doc.name}</h1>
      <div className="request-line">
        <select aria-label="Method" value={shown.method} onChange={(e) => edit({ method: e.target.value })}>
          {methods.map((method) => (
            <option key={method} value={method}>
              {method}
            </option>
          ))}
        </select>
        <input aria-label="URL" value={shown.url} onChange={(e) => edit({ url: e.target.value })} />
        <Button label="Save" disabled={draft === null} onClick={() => void save()} />
      </div>
    </div>
  );
}
