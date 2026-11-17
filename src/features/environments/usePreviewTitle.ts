import { useEffect, useState } from "react";
import type { RequestLocation } from "../../ipc/bindings";
import { previewTemplate } from "./api";

const PREVIEW_DEBOUNCE_MS = 200;

/** Tooltip text with variables substituted (secrets masked); `undefined` when nothing to show. */
export function usePreviewTitle(location: RequestLocation, text: string): string | undefined {
  const [preview, setPreview] = useState<string | undefined>();
  const key = location.scope === "scratch" ? "scratch" : location.collection_id;

  useEffect(() => {
    if (!text.includes("{{")) {
      setPreview(undefined);
      return;
    }
    let live = true;
    const timer = setTimeout(() => {
      previewTemplate(location, text).then(
        (result) => live && setPreview(result.text),
        () => live && setPreview(undefined),
      );
    }, PREVIEW_DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
    // `key` identifies the scope; the request path does not change the result.
  }, [key, text]);

  return preview;
}
