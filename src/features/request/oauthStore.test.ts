import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OAuthStatus, RequestContent } from "../../ipc/bindings";
import { useToastStore } from "../../ui/toastStore";
import * as api from "./oauthApi";
import { CANCELLED_TEXT, tabState, useOAuthStore } from "./oauthStore";

vi.mock("./oauthApi");

const location = { scope: "scratch" as const, id: "s1" };
const content = {} as RequestContent;
const status = (over: Partial<OAuthStatus> = {}): OAuthStatus => ({
  config_key: "k1", has_token: false, expires_at_ms: null, can_refresh: false, verify_tls: true, scope: "",
  auth_host: null, token_host: "t.example.com", plaintext: false, redirect_uri: null, ...over,
});
const tab = () => tabState(useOAuthStore.getState(), "t1");

beforeEach(() => {
  vi.resetAllMocks();
  useOAuthStore.setState({ byTab: {} });
  useToastStore.setState({ toasts: [] });
});

describe("oauthStore", () => {
  it("keeps the status read for a tab", async () => {
    vi.mocked(api.oauthStatus).mockResolvedValue(status({ has_token: true }));
    await useOAuthStore.getState().refresh("t1", location, content);
    expect(tab().status?.has_token).toBe(true);
  });

  it("marks the configuration incomplete without a toast when a background read fails", async () => {
    vi.mocked(api.oauthStatus).mockRejectedValue({ code: "invalid_input", message: "Unresolved: x" });
    const result = await useOAuthStore.getState().refresh("t1", location, content);
    expect(result).toBeNull();
    expect(tab().incomplete).toBe(true);
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("toasts a failed read when asked to report", async () => {
    vi.mocked(api.oauthStatus).mockRejectedValue({ code: "invalid_input", message: "Unresolved: x" });
    await useOAuthStore.getState().refresh("t1", location, content, true);
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });

  it("cancels the pending flow and forgets the tab on drop", () => {
    vi.mocked(api.oauthCancel).mockResolvedValue(null);
    vi.mocked(api.oauthAuthorize).mockReturnValue(new Promise(() => {}));
    void useOAuthStore.getState().authorize("t1", location, content, "k1");
    const flowId = tab().flowId;
    useOAuthStore.getState().drop("t1");
    expect(api.oauthCancel).toHaveBeenCalledWith(flowId);
    expect(useOAuthStore.getState().byTab.t1).toBeUndefined();
  });

  it("ignores a stale status answer", async () => {
    let first!: (s: OAuthStatus) => void;
    vi.mocked(api.oauthStatus)
      .mockReturnValueOnce(new Promise((r) => (first = r)))
      .mockResolvedValueOnce(status({ config_key: "new" }));
    const slow = useOAuthStore.getState().refresh("t1", location, content);
    await useOAuthStore.getState().refresh("t1", location, content);
    first(status({ config_key: "old" }));
    await slow;
    expect(tab().status?.config_key).toBe("new");
  });

  it("authorizes with a UUID flow id and the displayed config key", async () => {
    vi.mocked(api.oauthAuthorize).mockResolvedValue(status({ has_token: true }));
    await useOAuthStore.getState().authorize("t1", location, content, "k1");
    const [flowId, , , key] = vi.mocked(api.oauthAuthorize).mock.calls[0];
    expect(flowId).toMatch(/^[0-9a-f-]{36}$/);
    expect(key).toBe("k1");
    expect(tab()).toMatchObject({ flowId: null, status: { has_token: true } });
  });

  it("does not start a second flow while one is pending", async () => {
    vi.mocked(api.oauthAuthorize).mockReturnValue(new Promise(() => {}));
    void useOAuthStore.getState().authorize("t1", location, content, "k1");
    await useOAuthStore.getState().authorize("t1", location, content, "k1");
    expect(api.oauthAuthorize).toHaveBeenCalledTimes(1);
  });

  it("cancels the pending flow and shows Cancelled without a toast", async () => {
    let reject!: (e: unknown) => void;
    vi.mocked(api.oauthAuthorize).mockReturnValue(new Promise((_, r) => (reject = r)));
    vi.mocked(api.oauthCancel).mockResolvedValue(null);
    const flow = useOAuthStore.getState().authorize("t1", location, content, "k1");
    const flowId = tab().flowId;
    useOAuthStore.getState().cancel("t1");
    expect(api.oauthCancel).toHaveBeenCalledWith(flowId);
    reject({ code: "cancelled", message: "x" });
    await flow;
    expect(tab()).toMatchObject({ flowId: null, notice: CANCELLED_TEXT });
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("does nothing on cancel without a flow", () => {
    useOAuthStore.getState().cancel("t1");
    expect(api.oauthCancel).not.toHaveBeenCalled();
  });

  it("toasts other authorize errors", async () => {
    vi.mocked(api.oauthAuthorize).mockRejectedValue({ code: "conflict", message: "Changed." });
    await useOAuthStore.getState().authorize("t1", location, content, "k1");
    expect(tab().notice).toBeNull();
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });

  it("stores the status after clearing and toasts a failure", async () => {
    vi.mocked(api.oauthClear).mockResolvedValueOnce(status());
    await useOAuthStore.getState().clear("t1", location, content);
    expect(tab().status?.has_token).toBe(false);
    vi.mocked(api.oauthClear).mockRejectedValueOnce(new Error("x"));
    await useOAuthStore.getState().clear("t1", location, content);
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });
});
