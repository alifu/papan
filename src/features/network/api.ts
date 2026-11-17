import { commands } from "../../ipc/bindings";
import type { NetworkSettings, ProxyInput, ProxyMode } from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const readNetworkSettings = (): Promise<NetworkSettings> => commands.readNetworkSettings();
export const writeProxySettings = (proxy: ProxyInput): Promise<NetworkSettings> => commands.writeProxySettings(proxy);
export const addClientCertificate = (host: string, port: number | null): Promise<NetworkSettings | null> =>
  commands.addClientCertificate(host, port);
export const removeClientCertificate = (id: string): Promise<NetworkSettings> => commands.removeClientCertificate(id);
export const readProxyMode = (): Promise<ProxyMode> => commands.readProxyMode();
