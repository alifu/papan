import { commands, events } from "../../ipc/bindings";
import type {
  Capture,
  CaptureSummary,
  ReplayResult,
  WebhookCaptured,
  WebhookConfig,
  WebhookListener,
  WebhookStopped,
} from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const listWebhooks = (): Promise<WebhookListener[]> => commands.listWebhooks();
export const saveWebhook = (config: WebhookConfig): Promise<WebhookListener> => commands.saveWebhook(config);
export const deleteWebhook = (id: string): Promise<null> => commands.deleteWebhook(id);
export const startWebhook = (id: string): Promise<WebhookListener> => commands.startWebhook(id);
export const stopWebhook = (id: string): Promise<null> => commands.stopWebhook(id);
export const listCaptures = (id: string): Promise<CaptureSummary[]> => commands.listCaptures(id);
export const readCapture = (id: string, captureId: string): Promise<Capture> =>
  commands.readCapture(id, captureId);
export const clearCaptures = (id: string): Promise<null> => commands.clearCaptures(id);
export const replayCapture = (id: string, captureId: string): Promise<ReplayResult> =>
  commands.replayCapture(id, captureId);

export const subscribeCaptured = (onCaptured: (event: WebhookCaptured) => void): Promise<() => void> =>
  events.webhookCaptured.listen((event) => onCaptured(event.payload));
export const subscribeStopped = (onStopped: (event: WebhookStopped) => void): Promise<() => void> =>
  events.webhookStopped.listen((event) => onStopped(event.payload));
