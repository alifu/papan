import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Auth } from "../../ipc/bindings";
import * as envApi from "../environments/api";
import { AuthEditor } from "./AuthEditor";

vi.mock("../environments/api");
vi.mock("../collections/api");

const location = { scope: "scratch" as const, id: "s1" };

function Harness({ initial }: { initial: Auth }) {
  const [auth, setAuth] = useState(initial);
  return <AuthEditor auth={auth} onChange={setAuth} location={location} variables={[]} />;
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(envApi.readVariables).mockResolvedValue([]);
  vi.mocked(envApi.writeVariables).mockResolvedValue([]);
  vi.mocked(envApi.effectiveVariables).mockResolvedValue([]);
});

describe("AuthEditor secrets", () => {
  it("offers Save as secret variable only for a literal value", () => {
    render(<Harness initial={{ type: "bearer", token: "" }} />);
    expect(screen.queryByRole("button", { name: /Save as secret/ })).toBeNull();
  });

  it("replaces the literal with a reference after saving", async () => {
    render(<Harness initial={{ type: "bearer", token: "abc" }} />);
    await userEvent.click(screen.getByRole("button", { name: "Save as secret variable…" }));
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(screen.getByLabelText("Token")).toHaveValue("{{token}}"));
    expect(screen.queryByRole("button", { name: /Save as secret/ })).toBeNull();
  });

  it("marks a field that names an undefined variable", () => {
    render(<Harness initial={{ type: "basic", username: "{{who}}", password: "" }} />);
    const field = screen.getByLabelText("Username");
    expect(field).toHaveClass("has-unresolved");
    expect(field).toHaveAttribute("title", "Not defined: who");
  });
});
