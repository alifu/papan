import { commands, events } from "../../ipc/bindings";
import type {
  GrpcEvent,
  GrpcSchema,
  GrpcStarted,
  ProtoImport,
  RequestContent,
  RequestLocation,
  WsMessage,
} from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const grpcSchema = (location: RequestLocation, content: RequestContent, allowNetwork: boolean): Promise<GrpcSchema | null> =>
  commands.grpcSchema(location, content, allowNetwork);
export const grpcReflect = (location: RequestLocation, content: RequestContent, refresh: boolean): Promise<GrpcSchema> =>
  commands.grpcReflect(location, content, refresh);
export const grpcImportProto = (location: RequestLocation): Promise<ProtoImport> => commands.grpcImportProto(location);
export const grpcStart = (callId: string, location: RequestLocation, content: RequestContent): Promise<GrpcStarted> =>
  commands.grpcStart(callId, location, content);
export const grpcSend = (callId: string, location: RequestLocation, message: string): Promise<WsMessage> =>
  commands.grpcSend(callId, location, message);
export const grpcEnd = (callId: string): Promise<null> => commands.grpcEnd(callId);
export const grpcCancel = (callId: string): Promise<null> => commands.grpcCancel(callId);
export const grpcLog = (callId: string): Promise<WsMessage[]> => commands.grpcLog(callId);
export const grpcClearLog = (callId: string): Promise<null> => commands.grpcClearLog(callId);
/** One listener for every call; the store routes by `call_id`. Resolves to the unlisten function. */
export const listenGrpcEvents = (onEvent: (event: GrpcEvent) => void): Promise<() => void> =>
  events.grpcEvent.listen((e) => onEvent(e.payload));
