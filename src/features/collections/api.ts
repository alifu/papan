import { commands, events } from "../../ipc/bindings";
import type {
  CollectionTree,
  RequestContent,
  RequestDoc,
  RequestKind,
  RequestLocation,
  ScriptTarget,
  Scripts,
  TreeChange,
} from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const listCollections = (): Promise<CollectionTree[]> => commands.listCollections();
export const createCollection = (name: string): Promise<CollectionTree | null> =>
  commands.createCollection(name);
export const openCollection = (): Promise<CollectionTree | null> => commands.openCollection();
export const closeCollection = (collectionId: string): Promise<null> =>
  commands.closeCollection(collectionId);
export const readTree = (collectionId: string): Promise<CollectionTree> =>
  commands.readTree(collectionId);
export const createFolder = (collectionId: string, parent: string, name: string): Promise<TreeChange> =>
  commands.createFolder(collectionId, parent, name);
export const createRequest = (
  collectionId: string,
  parent: string,
  name: string,
  kind: RequestKind = "http",
): Promise<TreeChange> => commands.createRequest(collectionId, parent, name, kind);
export const renameNode = (collectionId: string, path: string, name: string): Promise<TreeChange> =>
  commands.renameNode(collectionId, path, name);
export const deleteNode = (collectionId: string, path: string): Promise<TreeChange> =>
  commands.deleteNode(collectionId, path);
export const moveNode = (
  collectionId: string,
  path: string,
  targetFolder: string,
  index: number,
): Promise<TreeChange> => commands.moveNode(collectionId, path, targetFolder, index);
export const readRequest = (location: RequestLocation): Promise<RequestDoc> =>
  commands.readRequest(location);
export const writeRequest = (location: RequestLocation, content: RequestContent): Promise<null> =>
  commands.writeRequest(location, content);

/** Calls `onChange` with the collection id whenever files change on disk. */
export async function subscribeCollectionChanged(
  onChange: (collectionId: string) => void,
): Promise<() => void> {
  return events.collectionChanged.listen((event) => onChange(event.payload.collection_id));
}
export const readScripts = (target: ScriptTarget): Promise<Scripts> => commands.readScripts(target);
export const writeScripts = (target: ScriptTarget, scripts: Scripts): Promise<null> =>
  commands.writeScripts(target, scripts);
