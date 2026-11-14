import { commands } from "../../ipc/bindings";
import type {
  EnvelopeOptions,
  RequestLocation,
  SoapTemplate,
  WsdlImport,
  WsdlRef,
  WsdlSource,
} from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
/** Resolves `null` when the file dialog was cancelled. Cancel a running import with `cancelImport`. */
export const importWsdl = (
  importId: string,
  location: RequestLocation,
  source: WsdlSource,
): Promise<WsdlImport | null> => commands.importWsdl(importId, location, source);
export const cancelImport = (importId: string): Promise<null> => commands.cancelRequest(importId);
export const readCachedWsdl = (location: RequestLocation, wsdl: WsdlRef): Promise<WsdlImport | null> =>
  commands.readCachedWsdl(location, wsdl);
export const soapEnvelope = (
  wsdlId: string,
  service: string,
  port: string,
  operation: string,
  options: EnvelopeOptions,
): Promise<SoapTemplate> => commands.soapEnvelope(wsdlId, service, port, operation, options);
