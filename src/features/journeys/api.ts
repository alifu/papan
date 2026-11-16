import { commands, events } from "../../ipc/bindings";
import type {
  Journey,
  JourneyRunEvent,
  JourneySummary,
  RunSummary,
} from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const listJourneys = (collectionId: string): Promise<JourneySummary[]> =>
  commands.listJourneys(collectionId);
export const readJourney = (
  collectionId: string,
  file: string,
): Promise<Journey> => commands.readJourney(collectionId, file);
export const saveJourney = (
  collectionId: string,
  file: string | null,
  journey: Journey,
): Promise<JourneySummary> => commands.saveJourney(collectionId, file, journey);
export const deleteJourney = (
  collectionId: string,
  file: string,
): Promise<null> => commands.deleteJourney(collectionId, file);
export const runJourney = (
  collectionId: string,
  file: string,
): Promise<RunSummary> => commands.runJourney(collectionId, file);
export const cancelJourney = (
  collectionId: string,
  file: string,
): Promise<null> => commands.cancelJourney(collectionId, file);
export const subscribeRun = (
  onEvent: (event: JourneyRunEvent) => void,
): Promise<() => void> =>
  events.journeyRunEvent.listen((event) => onEvent(event.payload));
