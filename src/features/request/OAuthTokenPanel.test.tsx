import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OAuthStatus, RequestContent } from "../../ipc/bindings";
import * as api from "./oauthApi";
import { OAuthTokenPanel, statusText } from "./OAuthTokenPanel";
import { useOAuthStore } from "./oauthStore";
import { useEnvStore } from "../environments/envStore";
import { useRequestStore } from "./requestStore";
import { act } from "@testing-library/react";

vi.mock("./oauthApi");

const location = { scope: "scratch" as const, id: "s1" };
const content = { auth: { type: "oauth2" } } as unknown as RequestContent;
const status = (over: Partial<OAuthStatus> = {}): OAuthStatus => ({
  config_key: "k1", has_token: false, expires_at_ms: null, can_refresh: false, verify_tls: true, scope: "",
  auth_host: "login.example.com", token_host: "token.example.com", plaintext: false,
  redirect_uri: "http://127.0.0.1:5000/callback", ...over,
});

beforeEach(() => {
  vi.resetAllMocks();
  useOAuthStore.setState({ byTab: {} });
  vi.mocked(api.oauthStatus).mockResolvedValue(status());
});

const mount = () => render(<OAuthTokenPanel tabId="t1" location={location} content={content} />);

describe("statusText", () => {
  const now = 1_000_000;
  it.each([
    [null, "No token"],
    [status(), "No token"],
    [status({ has_token: true }), "Token valid"],
    [status({ has_token: true, expires_at_ms: now - 1, can_refresh: true }), "Expired, will refresh"],
    [status({ has_token: true, expires_at_ms: now - 1 }), "Expired"],
    [status({ has_token: true, expires_at_ms: now + 29_000, can_refresh: true }), "Expired, will refresh"],
  ])("describes %#", (s, text) => expect(statusText(s, now)).toBe(text));

  it("shows the expiry time for a valid token", () => {
    expect(statusText(status({ has_token: true, expires_at_ms: now + 60_000 }), now)).toMatch(/^Token valid until \d/);
  });
});

describe("OAuthTokenPanel", () => {
  it("shows the status read from the backend", async () => {
    vi.mocked(api.oauthStatus).mockResolvedValue(status({ has_token: true, expires_at_ms: Date.now() + 3_600_000 }));
    mount();
    expect(await screen.findByText(/Token valid until/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear token" })).toBeEnabled();
  });

  it("names both hosts, the redirect URI and the http warning before authorizing", async () => {
    vi.mocked(api.oauthStatus).mockResolvedValue(status({ plaintext: true }));
    mount();
    await userEvent.click(screen.getByRole("button", { name: "Get new token" }));
    const dialog = await screen.findByRole("dialog", { name: "Get new token" });
    expect(dialog).toHaveTextContent("login.example.com");
    expect(dialog).toHaveTextContent("token.example.com");
    expect(dialog).toHaveTextContent("http://127.0.0.1:5000/callback");
    expect(screen.getByRole("alert")).toHaveTextContent("plain http");
    expect(api.oauthAuthorize).not.toHaveBeenCalled();
  });

  it("warns when TLS verification is off", async () => {
    vi.mocked(api.oauthStatus).mockResolvedValue(status({ verify_tls: false }));
    mount();
    await userEvent.click(screen.getByRole("button", { name: "Get new token" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "TLS certificate verification is off: the client secret and tokens are sent without certificate checks.",
    );
  });

  it("does not warn about TLS when verification is on", async () => {
    mount();
    await userEvent.click(screen.getByRole("button", { name: "Get new token" }));
    await screen.findByRole("dialog", { name: "Get new token" });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("omits the authorization host and warning for client credentials", async () => {
    vi.mocked(api.oauthStatus).mockResolvedValue(status({ auth_host: null, redirect_uri: null }));
    mount();
    await userEvent.click(screen.getByRole("button", { name: "Get new token" }));
    const dialog = await screen.findByRole("dialog", { name: "Get new token" });
    expect(dialog).not.toHaveTextContent("browser");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("authorizes with the displayed config key, then shows the new status", async () => {
    vi.mocked(api.oauthStatus).mockResolvedValue(status({ config_key: "shown" }));
    vi.mocked(api.oauthAuthorize).mockResolvedValue(status({ has_token: true }));
    mount();
    await userEvent.click(screen.getByRole("button", { name: "Get new token" }));
    await userEvent.click(await screen.findByRole("button", { name: "Get token" }));
    expect(vi.mocked(api.oauthAuthorize).mock.calls[0][3]).toBe("shown");
    expect(await screen.findByText("Token valid")).toBeInTheDocument();
  });

  it("does not authorize when the dialog is dismissed", async () => {
    mount();
    await userEvent.click(screen.getByRole("button", { name: "Get new token" }));
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(api.oauthAuthorize).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("shows a spinner text and cancels the pending flow", async () => {
    let reject!: (e: unknown) => void;
    vi.mocked(api.oauthAuthorize).mockReturnValue(new Promise((_, r) => (reject = r)));
    vi.mocked(api.oauthCancel).mockResolvedValue(null);
    mount();
    await userEvent.click(screen.getByRole("button", { name: "Get new token" }));
    await userEvent.click(await screen.findByRole("button", { name: "Get token" }));
    expect(screen.getByRole("status")).toHaveTextContent("Waiting for the browser");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.oauthCancel).toHaveBeenCalled();
    reject({ code: "cancelled", message: "x" });
    expect(await screen.findByText("Cancelled")).toBeInTheDocument();
  });

  it("clears the token", async () => {
    vi.mocked(api.oauthStatus).mockResolvedValue(status({ has_token: true }));
    vi.mocked(api.oauthClear).mockResolvedValue(status());
    mount();
    const clear = screen.getByRole("button", { name: "Clear token" });
    await vi.waitFor(() => expect(clear).toBeEnabled());
    await userEvent.click(clear);
    expect(await screen.findByText("No token")).toBeInTheDocument();
  });

  it("shows Configuration incomplete when the read fails, without a toast", async () => {
    vi.mocked(api.oauthStatus).mockRejectedValue({ code: "invalid_input", message: "x" });
    mount();
    expect(await screen.findByText("Configuration incomplete")).toBeInTheDocument();
  });

  it("reads the status again when the effective variables change", async () => {
    mount();
    await vi.waitFor(() => expect(api.oauthStatus).toHaveBeenCalledTimes(1));
    act(() => useEnvStore.setState({ effective: { scratch: { location, variables: [] } } } as never));
    await vi.waitFor(() => expect(api.oauthStatus).toHaveBeenCalledTimes(2));
  });

  it("reads the status again when a send finishes", async () => {
    mount();
    await vi.waitFor(() => expect(api.oauthStatus).toHaveBeenCalledTimes(1));
    act(() => useRequestStore.setState({ byTab: { t1: { sending: true } } } as never));
    act(() => useRequestStore.setState({ byTab: { t1: { sending: false } } } as never));
    await vi.waitFor(() => expect(api.oauthStatus).toHaveBeenCalledTimes(2));
  });

  it("turns Expired at the skew moment without another status call", async () => {
    vi.mocked(api.oauthStatus).mockResolvedValue(
      status({ has_token: true, expires_at_ms: Date.now() + 30_400, can_refresh: true }),
    );
    mount();
    expect(await screen.findByText(/Token valid until/)).toBeInTheDocument();
    expect(await screen.findByText("Expired, will refresh", {}, { timeout: 2000 })).toBeInTheDocument();
    expect(api.oauthStatus).toHaveBeenCalledTimes(1);
  });
});
