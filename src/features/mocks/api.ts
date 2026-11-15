import { commands, events } from "../../ipc/bindings";
import type {
  ImportedMock,
  MockEnvironment,
  MockImportFormat,
  MockLogEntry,
  MockLogged,
  MockSummary,
} from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const listMocks = (collectionId: string): Promise<MockSummary[]> =>
  commands.listMocks(collectionId);
export const readMock = (
  collectionId: string,
  file: string,
): Promise<MockEnvironment> => commands.readMock(collectionId, file);
export const saveMock = (
  collectionId: string,
  file: string | null,
  mock: MockEnvironment,
): Promise<MockSummary> => commands.saveMock(collectionId, file, mock);
export const mockFromCollection = (
  collectionId: string,
): Promise<MockSummary> => commands.mockFromCollection(collectionId);
export const importMock = (
  collectionId: string,
  format: MockImportFormat,
): Promise<ImportedMock | null> => commands.importMock(collectionId, format);
export const exportMock = (
  collectionId: string,
  file: string,
): Promise<string | null> => commands.exportMock(collectionId, file);
export const deleteMock = (collectionId: string, file: string): Promise<null> =>
  commands.deleteMock(collectionId, file);
export const startMock = (
  collectionId: string,
  file: string,
): Promise<string> => commands.startMock(collectionId, file);
export const stopMock = (collectionId: string, file: string): Promise<null> =>
  commands.stopMock(collectionId, file);
export const mockLog = (
  collectionId: string,
  file: string,
): Promise<MockLogEntry[]> => commands.mockLog(collectionId, file);
export const clearMockLog = (
  collectionId: string,
  file: string,
): Promise<null> => commands.clearMockLog(collectionId, file);
export const subscribeLogged = (
  onLogged: (event: MockLogged) => void,
): Promise<() => void> =>
  events.mockLogged.listen((event) => onLogged(event.payload));
