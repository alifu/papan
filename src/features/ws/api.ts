import { commands, events } from "../../ipc/bindings";
import type { RequestContent, RequestLocation, WsEvent, WsMessage, WsOpened, WsOutgoing } from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const wsConnect = (connectionId: string, location: RequestLocation, content: RequestContent): Promise<WsOpened> =>
  commands.wsConnect(connectionId, location, content);
export const wsSend = (connectionId: string, location: RequestLocation, message: WsOutgoing): Promise<WsMessage> =>
  commands.wsSend(connectionId, location, message);
export const wsDisconnect = (connectionId: string, code: number | null, reason: string): Promise<null> =>
  commands.wsDisconnect(connectionId, code, reason);
export const wsLog = (connectionId: string): Promise<WsMessage[]> => commands.wsLog(connectionId);
export const wsClearLog = (connectionId: string): Promise<null> => commands.wsClearLog(connectionId);
/** One listener for every connection; the store routes by `connection_id`. Resolves to the unlisten function. */
export const listenWsEvents = (onEvent: (event: WsEvent) => void): Promise<() => void> =>
  events.wsEvent.listen((e) => onEvent(e.payload));
export const graphqlSubscribe = (connectionId: string, location: RequestLocation, content: RequestContent): Promise<WsOpened> =>
  commands.graphqlSubscribe(connectionId, location, content);
export const graphqlUnsubscribe = (connectionId: string): Promise<null> => commands.graphqlUnsubscribe(connectionId);
