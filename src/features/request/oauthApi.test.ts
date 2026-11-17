import { describe, expect, it, vi } from "vitest";
import type { RequestContent } from "../../ipc/bindings";
import { commands } from "../../ipc/bindings";
import * as api from "./oauthApi";

vi.mock("../../ipc/bindings", () => ({
  commands: { oauthStatus: vi.fn(), oauthAuthorize: vi.fn(), oauthCancel: vi.fn(), oauthClear: vi.fn() },
}));

const location = { scope: "scratch" as const, id: "s1" };
const content = {} as RequestContent;

describe("oauthApi", () => {
  it("forwards each call to its command", async () => {
    await api.oauthStatus(location, content);
    await api.oauthAuthorize("f", location, content, "k");
    await api.oauthCancel("f");
    await api.oauthClear(location, content);
    expect(commands.oauthStatus).toHaveBeenCalledWith(location, content);
    expect(commands.oauthAuthorize).toHaveBeenCalledWith("f", location, content, "k");
    expect(commands.oauthCancel).toHaveBeenCalledWith("f");
    expect(commands.oauthClear).toHaveBeenCalledWith(location, content);
  });
});
