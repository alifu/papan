import { beforeEach, describe, expect, it, vi } from "vitest";
import { emptyContent } from "../request/content";
import { commands, events } from "../../ipc/bindings";
import * as api from "./api";
import * as scratchApi from "../scratch/api";

vi.mock("../../ipc/bindings", () => {
  const fn = () => vi.fn().mockResolvedValue("ok");
  return {
    commands: {
      listCollections: fn(), createCollection: fn(), openCollection: fn(), closeCollection: fn(),
      readTree: fn(), createFolder: fn(), createRequest: fn(), renameNode: fn(), deleteNode: fn(),
      moveNode: fn(), readRequest: fn(), writeRequest: fn(), listScratch: fn(), createScratch: fn(),
      renameScratch: fn(), deleteScratch: fn(), moveScratchToCollection: fn(),
    },
    events: { collectionChanged: { listen: vi.fn() } },
  };
});

const loc = { scope: "scratch", id: "s" } as const;

describe("api wrappers", () => {
  beforeEach(() => vi.clearAllMocks());

  it("forward arguments to the matching command", async () => {
    await api.listCollections();
    await api.createCollection("n");
    await api.openCollection();
    await api.closeCollection("c");
    await api.readTree("c");
    await api.createFolder("c", "p", "n");
    await api.createRequest("c", "p", "n", "graphql");
    await api.renameNode("c", "p", "n");
    await api.deleteNode("c", "p");
    await api.moveNode("c", "p", "t", 2);
    await api.readRequest(loc);
    await api.writeRequest(loc, emptyContent());
    await scratchApi.listScratch();
    await scratchApi.createScratch();
    await scratchApi.renameScratch("s", "n");
    await scratchApi.deleteScratch("s");
    await scratchApi.moveScratchToCollection("s", "c", "f");

    expect(commands.createCollection).toHaveBeenCalledWith("n");
    expect(commands.moveNode).toHaveBeenCalledWith("c", "p", "t", 2);
    expect(commands.writeRequest).toHaveBeenCalledWith(loc, emptyContent());
    expect(commands.moveScratchToCollection).toHaveBeenCalledWith("s", "c", "f");
    expect(commands.listScratch).toHaveBeenCalledTimes(1);
  });

  it("subscribeCollectionChanged unwraps the event payload", async () => {
    const unlisten = () => {};
    vi.mocked(events.collectionChanged.listen).mockImplementation(async (cb) => {
      cb({ payload: { collection_id: "c9" } } as never);
      return unlisten;
    });
    const onChange = vi.fn();

    const result = await api.subscribeCollectionChanged(onChange);

    expect(onChange).toHaveBeenCalledWith("c9");
    expect(result).toBe(unlisten);
  });
});
