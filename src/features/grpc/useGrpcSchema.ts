import { useEffect } from "react";
import type { RequestContent, RequestLocation } from "../../ipc/bindings";
import { grpcSettingsOf } from "./grpcModel";
import { useGrpcStore } from "./grpcStore";

export const SCHEMA_DEBOUNCE_MS = 500;

/** Loads the schema (cache and protos only, never the network) when the request opens and, debounced, when the URL or the proto setup changes. */
export function useGrpcSchema(tabId: string, location: RequestLocation, content: RequestContent): void {
  const grpc = grpcSettingsOf(content);
  const inputs = JSON.stringify([content.url, grpc.source, grpc.protos, grpc.import_paths]);
  useEffect(() => {
    const timer = setTimeout(() => void useGrpcStore.getState().loadSchema(tabId, location, content), SCHEMA_DEBOUNCE_MS);
    return () => clearTimeout(timer);
    // `inputs` captures every field that changes the schema; `content` itself changes on every keystroke.
  }, [tabId, inputs]);
}
