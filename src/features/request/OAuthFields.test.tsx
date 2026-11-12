import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Auth } from "../../ipc/bindings";
import * as envApi from "../environments/api";
import { AuthEditor } from "./AuthEditor";
import { OAUTH_DEFAULT, PORT_ERROR } from "./OAuthFields";

vi.mock("../environments/api");
vi.mock("../collections/api");

let latest: Auth = { type: "none" };
function Harness({ initial }: { initial: Auth }) {
  const [auth, setAuth] = useState(initial);
  latest = auth;
  return <AuthEditor auth={auth} onChange={setAuth} location={{ scope: "scratch", id: "s" }} variables={[]} />;
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(envApi.effectiveVariables).mockResolvedValue([]);
});

describe("Digest and OAuth 2 auth", () => {
  it("switches to Digest and edits the username", async () => {
    render(<Harness initial={{ type: "none" }} />);
    await userEvent.selectOptions(screen.getByLabelText("Auth type"), "digest");
    await userEvent.type(screen.getByLabelText("Username"), "al");
    expect(latest).toEqual({ type: "digest", username: "al", password: "" });
  });

  it("switches to OAuth 2 and hides the authorization URL for client credentials", async () => {
    render(<Harness initial={{ type: "none" }} />);
    await userEvent.selectOptions(screen.getByLabelText("Auth type"), "oauth2");
    expect(latest).toEqual(OAUTH_DEFAULT);
    expect(screen.queryByLabelText("Authorization URL")).toBeNull();
    expect(screen.queryByLabelText(/Redirect port/)).toBeNull();
  });

  it("edits fields and shows the authorization code extras", async () => {
    render(<Harness initial={OAUTH_DEFAULT} />);
    await userEvent.selectOptions(screen.getByLabelText("Grant type"), "authorization_code");
    await userEvent.type(screen.getByLabelText("Authorization URL"), "https://a");
    await userEvent.type(screen.getByLabelText("Token URL"), "https://t");
    await userEvent.type(screen.getByLabelText("Client ID"), "id");
    await userEvent.type(screen.getByLabelText("Scope"), "read");
    await userEvent.selectOptions(screen.getByLabelText("Client authentication"), "body");
    expect(latest).toMatchObject({
      grant: "authorization_code", auth_url: "https://a", token_url: "https://t", client_id: "id", scope: "read", client_auth: "body",
    });
  });

  it("keeps the client secret in a password input", () => {
    render(<Harness initial={{ ...OAUTH_DEFAULT, client_secret: "s" }} />);
    expect(screen.getByLabelText("Client secret")).toHaveAttribute("type", "password");
  });

  it("validates the redirect port", async () => {
    render(<Harness initial={{ ...OAUTH_DEFAULT, grant: "authorization_code" }} />);
    const port = screen.getByLabelText(/Redirect port/);
    await userEvent.clear(port);
    await userEvent.type(port, "80");
    expect(screen.getByRole("alert")).toHaveTextContent(PORT_ERROR);
    await userEvent.clear(port);
    await userEvent.type(port, "8080");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(latest).toMatchObject({ redirect_port: 8080 });
  });
});
