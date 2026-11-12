import { useEffect } from "react";
import type { EffectiveVariable, RequestLocation } from "../../ipc/bindings";
import { scopeKey, useEnvStore } from "./envStore";

/**
 * Effective variables for `location`'s scope, fetched on mount and kept fresh by the store.
 * `undefined` until the first answer arrives, so nothing is flagged as unresolved too early.
 */
export function useEffectiveVariables(location: RequestLocation): readonly EffectiveVariable[] | undefined {
  const key = scopeKey(location);
  useEffect(() => {
    void useEnvStore.getState().loadEffective(location);
    // `key` identifies the scope; the request path inside it does not change the result.
  }, [key]);
  return useEnvStore((s) => s.effective[key]?.variables);
}
