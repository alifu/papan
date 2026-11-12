import { create } from "zustand";
import type { NetworkSettings, ProxyInput, ProxyMode } from "../../ipc/bindings";
import { reportError } from "../../ui/toastStore";
import * as api from "./api";

export interface NetworkState {
  readonly settings: NetworkSettings | null;
  /** Proxy mode for the URL bar badge; read without opening the vault. */
  readonly proxyMode: ProxyMode | null;
  loadProxyMode: () => Promise<void>;
  load: () => Promise<void>;
  saveProxy: (proxy: ProxyInput) => Promise<boolean>;
  addCertificate: (host: string, port: number | null) => Promise<boolean>;
  removeCertificate: (id: string) => Promise<boolean>;
}

export const useNetworkStore = create<NetworkState>()((set) => {
  // Every call takes a ticket; an answer is applied only if no newer call started since.
  let latest = 0;
  /** Runs `call`; a non-null result replaces the settings. Errors become toasts. */
  const apply = async (call: () => Promise<NetworkSettings | null>): Promise<boolean> => {
    const ticket = ++latest;
    try {
      const settings = await call();
      if (settings && ticket === latest) set({ settings, proxyMode: settings.proxy.mode });
      return settings !== null;
    } catch (err) {
      reportError(err);
      return false;
    }
  };
  return {
    settings: null,
    proxyMode: null,
    loadProxyMode: async () => {
      const ticket = ++latest;
      try {
        const proxyMode = await api.readProxyMode();
        if (ticket === latest) set({ proxyMode });
      } catch (err) {
        reportError(err);
      }
    },
    load: async () => void (await apply(api.readNetworkSettings)),
    saveProxy: (proxy) => apply(() => api.writeProxySettings(proxy)),
    addCertificate: (host, port) => apply(() => api.addClientCertificate(host, port)),
    removeCertificate: (id) => apply(() => api.removeClientCertificate(id)),
  };
});
