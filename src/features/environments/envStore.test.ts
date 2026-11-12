import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EffectiveVariable, RequestLocation } from "../../ipc/bindings";
import * as collectionsApi from "../collections/api";
import * as api from "./api";
import { scopeKey, useEnvStore } from "./envStore";

vi.mock("./api");
vi.mock("../collections/api");

const scratch: RequestLocation = { scope: "scratch", id: "s1" };
const inCollection: RequestLocation = { scope: "collection", collection_id: "c1", path: "a.http.yaml" };
const list = { environments: [{ env: "dev", name: "Dev", invalid: false }], selected: null };
const variable = (name: string): EffectiveVariable => ({ name, value: "v", source: "global", secret: false, resolved: true });

beforeEach(() => {
  vi.resetAllMocks();
  useEnvStore.setState(useEnvStore.getInitialState(), true);
});

describe("envStore", () => {
  it("keys scratch requests together and collection requests by collection", () => {
    expect(scopeKey(scratch)).toBe("scratch");
    expect(scopeKey(inCollection)).toBe("c1");
  });

  it("loads environments per collection", async () => {
    vi.mocked(api.listEnvironments).mockResolvedValue(list);
    await useEnvStore.getState().loadEnvironments("c1");
    expect(useEnvStore.getState().byCollection.c1).toEqual(list);
  });

  it("caches effective variables and refetches them all on select", async () => {
    vi.mocked(api.effectiveVariables).mockResolvedValue([variable("a")]);
    vi.mocked(api.selectEnvironment).mockResolvedValue(null);
    vi.mocked(api.listEnvironments).mockResolvedValue(list);
    await useEnvStore.getState().loadEffective(scratch);
    await useEnvStore.getState().loadEffective(inCollection);
    await useEnvStore.getState().loadEnvironments("c1");
    vi.mocked(api.effectiveVariables).mockClear();

    await useEnvStore.getState().select("c1", "dev");

    expect(api.selectEnvironment).toHaveBeenCalledWith("c1", "dev");
    expect(api.effectiveVariables).toHaveBeenCalledTimes(2);
    expect(useEnvStore.getState().byCollection.c1?.selected).toBe("dev");
  });

  it("does not change the selection when the backend refuses", async () => {
    vi.mocked(api.listEnvironments).mockResolvedValue(list);
    vi.mocked(api.selectEnvironment).mockRejectedValue({ code: "io", message: "no" });
    await useEnvStore.getState().loadEnvironments("c1");
    await useEnvStore.getState().select("c1", "dev");
    expect(useEnvStore.getState().byCollection.c1?.selected).toBeNull();
  });

  it("replaces the list after create, rename and delete and returns whether it worked", async () => {
    vi.mocked(api.createEnvironment).mockResolvedValue(list);
    vi.mocked(api.renameEnvironment).mockRejectedValue({ code: "io", message: "no" });
    vi.mocked(api.deleteEnvironment).mockResolvedValue({ environments: [], selected: null });
    const s = useEnvStore.getState();
    expect(await s.createEnvironment("c1", "Dev")).toBe(true);
    expect(await s.renameEnvironment("c1", "dev", "X")).toBe(false);
    expect(await s.deleteEnvironment("c1", "dev")).toBe(true);
    expect(useEnvStore.getState().byCollection.c1?.environments).toEqual([]);
  });

  it("tracks the secret storage status and resets it", async () => {
    vi.mocked(api.secretStorageStatus).mockResolvedValue({ mode: "damaged", message: "bad" });
    vi.mocked(api.resetSecretStorage).mockResolvedValue({ mode: "keychain", message: null });
    await useEnvStore.getState().loadStorage();
    expect(useEnvStore.getState().storage?.mode).toBe("damaged");
    await useEnvStore.getState().resetStorage();
    expect(useEnvStore.getState().storage?.mode).toBe("keychain");
  });

  it("reloads on CollectionChanged", async () => {
    let fire: (id: string) => void = () => {};
    vi.mocked(collectionsApi.subscribeCollectionChanged).mockImplementation((cb) => {
      fire = cb;
      return Promise.resolve(() => {});
    });
    vi.mocked(api.listEnvironments).mockResolvedValue(list);
    await useEnvStore.getState().subscribe();
    fire("c1");
    await vi.waitFor(() => expect(useEnvStore.getState().byCollection.c1).toEqual(list));
  });
});
