import { create } from "zustand";
import type {
  EffectiveVariable,
  EnvironmentList,
  RequestLocation,
  SecretStorageStatus,
} from "../../ipc/bindings";
import { reportError } from "../../ui/toastStore";
import { subscribeCollectionChanged } from "../collections/api";
import * as api from "./api";

/** Effective variables are cached per scope: all scratch requests share one, a collection has its own. */
export const scopeKey = (location: RequestLocation): string =>
  location.scope === "scratch" ? "scratch" : location.collection_id;

interface EffectiveEntry {
  readonly location: RequestLocation;
  readonly variables: readonly EffectiveVariable[];
}

export interface EnvState {
  readonly byCollection: Readonly<Record<string, EnvironmentList>>;
  readonly effective: Readonly<Record<string, EffectiveEntry>>;
  readonly storage: SecretStorageStatus | null;
  loadEnvironments: (collectionId: string) => Promise<void>;
  createEnvironment: (collectionId: string, name: string) => Promise<boolean>;
  renameEnvironment: (collectionId: string, env: string, name: string) => Promise<boolean>;
  deleteEnvironment: (collectionId: string, env: string) => Promise<boolean>;
  select: (collectionId: string, env: string | null) => Promise<void>;
  /** Fetches the effective variables for `location` and remembers it for later refetches. */
  loadEffective: (location: RequestLocation) => Promise<void>;
  /** Refetches every cached scope (after a select, a write or an outside change). */
  refreshEffective: () => Promise<void>;
  loadStorage: () => Promise<void>;
  resetStorage: () => Promise<void>;
  subscribe: () => Promise<() => void>;
}

export const useEnvStore = create<EnvState>()((set, get) => {
  const attempt = async <T,>(call: () => Promise<T>): Promise<T | null> => {
    try {
      return await call();
    } catch (err) {
      reportError(err);
      return null;
    }
  };

  const putList = (collectionId: string, list: EnvironmentList) =>
    set({ byCollection: { ...get().byCollection, [collectionId]: list } });

  const mutate = async (collectionId: string, call: () => Promise<EnvironmentList>): Promise<boolean> => {
    const list = await attempt(call);
    if (!list) return false;
    putList(collectionId, list);
    await get().refreshEffective();
    return true;
  };

  return {
    byCollection: {},
    effective: {},
    storage: null,
    loadEnvironments: async (collectionId) => {
      const list = await attempt(() => api.listEnvironments(collectionId));
      if (list) putList(collectionId, list);
    },
    createEnvironment: (collectionId, name) => mutate(collectionId, () => api.createEnvironment(collectionId, name)),
    renameEnvironment: (collectionId, env, name) =>
      mutate(collectionId, () => api.renameEnvironment(collectionId, env, name)),
    deleteEnvironment: (collectionId, env) => mutate(collectionId, () => api.deleteEnvironment(collectionId, env)),
    select: async (collectionId, env) => {
      try {
        await api.selectEnvironment(collectionId, env);
      } catch (err) {
        reportError(err);
        return;
      }
      const current = get().byCollection[collectionId];
      if (current) putList(collectionId, { ...current, selected: env });
      await get().refreshEffective();
    },
    loadEffective: async (location) => {
      const variables = await attempt(() => api.effectiveVariables(location));
      if (!variables) return;
      set({ effective: { ...get().effective, [scopeKey(location)]: { location, variables } } });
    },
    refreshEffective: async () => {
      await Promise.all(Object.values(get().effective).map((entry) => get().loadEffective(entry.location)));
    },
    loadStorage: async () => {
      const storage = await attempt(api.secretStorageStatus);
      if (storage) set({ storage });
    },
    resetStorage: async () => {
      const storage = await attempt(api.resetSecretStorage);
      if (storage) set({ storage });
    },
    subscribe: () =>
      subscribeCollectionChanged((collectionId) => {
        void get().loadEnvironments(collectionId);
        void get().refreshEffective();
      }),
  };
});
