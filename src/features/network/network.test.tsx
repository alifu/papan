import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NetworkSettings } from "../../ipc/bindings";
import { useToastStore } from "../../ui/toastStore";
import * as api from "./api";
import { NetworkDialog } from "./NetworkDialog";
import { useNetworkStore } from "./networkStore";
import { parsePort } from "./CertificateList";
import { validProxyUrl } from "./ProxyForm";

vi.mock("./api");

const settings: NetworkSettings = {
  proxy: { mode: "system", url: "", username: "", has_password: false, bypass: [] },
  certificates: [
    { id: "c1", host: "api.bank", port: 8443, cert_name: "c.pem", key_name: "k.key", added_at_ms: 0, missing: false },
    { id: "c2", host: "gone.test", port: null, cert_name: "", key_name: "", added_at_ms: null, missing: true },
  ],
  storage: { mode: "session_only", message: "Session only" },
};

beforeEach(() => {
  vi.resetAllMocks();
  useNetworkStore.setState({ settings: null });
  useToastStore.setState({ toasts: [] });
  vi.mocked(api.readNetworkSettings).mockResolvedValue(settings);
});

describe("validators", () => {
  it("accepts bare http(s) proxy URLs only", () => {
    expect(validProxyUrl("http://proxy:3128")).toBe(true);
    expect(validProxyUrl("socks5://p")).toBe(false);
    expect(validProxyUrl("http://u:p@proxy")).toBe(false);
    expect(validProxyUrl("http://proxy/path")).toBe(false);
    expect(validProxyUrl("http://proxy?q=1")).toBe(false);
    expect(validProxyUrl("nope")).toBe(false);
  });

  it("parses ports", () => {
    expect(parsePort("")).toBeNull();
    expect(parsePort("443")).toBe(443);
    expect(parsePort("0")).toBeUndefined();
    expect(parsePort("70000")).toBeUndefined();
    expect(parsePort("x")).toBeUndefined();
  });
});

describe("networkStore", () => {
  it("reads the proxy mode without loading settings", async () => {
    vi.mocked(api.readProxyMode).mockResolvedValue("custom");
    await useNetworkStore.getState().loadProxyMode();
    expect(useNetworkStore.getState().proxyMode).toBe("custom");
    expect(api.readNetworkSettings).not.toHaveBeenCalled();
  });

  it("ignores a stale load that lands after a save", async () => {
    let finishLoad: (s: NetworkSettings) => void = () => {};
    vi.mocked(api.readNetworkSettings).mockReturnValue(new Promise((r) => (finishLoad = r)));
    const saved = { ...settings, proxy: { ...settings.proxy, mode: "none" as const } };
    vi.mocked(api.writeProxySettings).mockResolvedValue(saved);
    const loading = useNetworkStore.getState().load();
    await useNetworkStore.getState().saveProxy({ mode: "none", url: "", username: "", password: { op: "keep" }, bypass: [] });
    finishLoad(settings);
    await loading;
    expect(useNetworkStore.getState().settings).toBe(saved);
    expect(useNetworkStore.getState().proxyMode).toBe("none");
  });

  it("toasts an error and keeps the old settings", async () => {
    useNetworkStore.setState({ settings });
    vi.mocked(api.writeProxySettings).mockRejectedValue({ code: "internal", message: "not available yet" });
    const ok = await useNetworkStore.getState().saveProxy({ mode: "none", url: "", username: "", password: { op: "keep" }, bypass: [] });
    expect(ok).toBe(false);
    expect(useNetworkStore.getState().settings).toBe(settings);
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });

  it("treats a cancelled certificate dialog as no change", async () => {
    vi.mocked(api.addClientCertificate).mockResolvedValue(null);
    expect(await useNetworkStore.getState().addCertificate("h", null)).toBe(false);
  });
});

describe("NetworkDialog", () => {
  it("shows the storage warning, certificates and missing rows", async () => {
    render(<NetworkDialog onClose={vi.fn()} />);
    expect(await screen.findByText("Session only")).toBeInTheDocument();
    expect(screen.getByText("api.bank")).toBeInTheDocument();
    expect(screen.getByText("Missing. Import it again.")).toBeInTheDocument();
  });

  it("saves a custom proxy with a write-only password", async () => {
    vi.mocked(api.writeProxySettings).mockResolvedValue({ ...settings, proxy: { ...settings.proxy, mode: "custom", has_password: true } });
    render(<NetworkDialog onClose={vi.fn()} />);
    await userEvent.click(await screen.findByLabelText("Custom proxy"));
    await userEvent.type(screen.getByLabelText("Proxy URL"), "http://proxy:3128");
    await userEvent.type(screen.getByLabelText("Username"), "al");
    await userEvent.type(screen.getByLabelText("Password"), "pw");
    await userEvent.type(screen.getByLabelText(/Bypass hosts/), "localhost{enter}*.internal");
    await userEvent.click(screen.getByRole("button", { name: "Save proxy" }));
    expect(api.writeProxySettings).toHaveBeenCalledWith({
      mode: "custom", url: "http://proxy:3128", username: "al", password: { op: "set", value: "pw" }, bypass: ["localhost", "*.internal"],
    });
    await vi.waitFor(() => expect(screen.getByLabelText("Password")).toHaveValue(""));
    expect(screen.getByLabelText("Password")).toHaveAttribute("placeholder", "Stored");
  });

  it("sends clear for the password and rejects a bad URL", async () => {
    const custom = { ...settings, proxy: { ...settings.proxy, mode: "custom" as const, url: "http://p", has_password: true } };
    useNetworkStore.setState({ settings: custom });
    vi.mocked(api.readNetworkSettings).mockResolvedValue(custom);
    vi.mocked(api.writeProxySettings).mockResolvedValue(settings);
    render(<NetworkDialog onClose={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: "Clear password" }));
    await userEvent.click(screen.getByRole("button", { name: "Save proxy" }));
    expect(api.writeProxySettings).toHaveBeenCalledWith(expect.objectContaining({ password: { op: "clear" } }));
    await userEvent.clear(screen.getByLabelText("Proxy URL"));
    await userEvent.type(screen.getByLabelText("Proxy URL"), "ftp://x");
    await userEvent.click(screen.getByRole("button", { name: "Save proxy" }));
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });

  it("rejects a half-typed URL even in system mode", async () => {
    render(<NetworkDialog onClose={vi.fn()} />);
    await userEvent.click(await screen.findByLabelText("Custom proxy"));
    await userEvent.type(screen.getByLabelText("Proxy URL"), "ht");
    await userEvent.click(screen.getByLabelText("Use system proxy"));
    await userEvent.click(screen.getByRole("button", { name: "Save proxy" }));
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(api.writeProxySettings).not.toHaveBeenCalled();
  });

  it("adds a certificate for a host and port", async () => {
    vi.mocked(api.addClientCertificate).mockResolvedValue(settings);
    render(<NetworkDialog onClose={vi.fn()} />);
    await userEvent.type(await screen.findByLabelText("Host"), "a.test");
    await userEvent.type(screen.getByLabelText(/Port/), "99999");
    await userEvent.click(screen.getByRole("button", { name: "Add…" }));
    expect(screen.getByRole("alert")).toHaveTextContent(/Port must be/);
    await userEvent.clear(screen.getByLabelText(/Port/));
    await userEvent.type(screen.getByLabelText(/Port/), "8443");
    await userEvent.click(screen.getByRole("button", { name: "Add…" }));
    expect(api.addClientCertificate).toHaveBeenCalledWith("a.test", 8443);
  });

  it("requires a host and confirms removal", async () => {
    vi.mocked(api.removeClientCertificate).mockResolvedValue({ ...settings, certificates: [] });
    render(<NetworkDialog onClose={vi.fn()} />);
    await userEvent.click(await screen.findByRole("button", { name: "Add…" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a host name.");
    await userEvent.click(screen.getByRole("button", { name: "Remove certificate api.bank" }));
    await userEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(api.removeClientCertificate).toHaveBeenCalledWith("c1");
  });
});
