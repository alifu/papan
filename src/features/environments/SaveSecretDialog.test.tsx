import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import { useEnvStore } from "./envStore";
import { SaveSecretDialog, targetScope } from "./SaveSecretDialog";

vi.mock("./api");
vi.mock("../collections/api");

const location = { scope: "collection" as const, collection_id: "c1", path: "a.http.yaml" };
const existing = { name: "base", value: "x", current: null, enabled: true, secret: false, has_secret: false };

beforeEach(() => {
  vi.resetAllMocks();
  useEnvStore.setState(useEnvStore.getInitialState(), true);
  vi.mocked(api.readVariables).mockResolvedValue([existing]);
  vi.mocked(api.writeVariables).mockResolvedValue([]);
});

describe("SaveSecretDialog", () => {
  it("targets the selected environment, else the collection, else globals", () => {
    expect(targetScope({ scope: "scratch", id: "s" }).scope).toEqual({ scope: "global" });
    expect(targetScope(location).scope).toEqual({ scope: "collection", collection_id: "c1" });
    useEnvStore.setState({ byCollection: { c1: { environments: [{ env: "dev", name: "Dev", invalid: false }], selected: "dev" } } });
    expect(targetScope(location)).toEqual({
      scope: { scope: "environment", collection_id: "c1", env: "dev" },
      label: 'Environment "Dev"',
    });
  });

  it("keeps existing rows, stores the secret and returns the reference", async () => {
    const onSaved = vi.fn();
    render(<SaveSecretDialog location={location} secret="abc" defaultName="token" onSaved={onSaved} onClose={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(onSaved).toHaveBeenCalledWith("{{token}}"));
    const [, inputs] = vi.mocked(api.writeVariables).mock.calls[0] ?? [];
    expect(inputs?.map((i) => i.name)).toEqual(["base", "token"]);
    expect(inputs?.[1]).toMatchObject({ secret: true, value: "", secret_edit: { op: "set", value: "abc" } });
  });

  it("refuses a duplicate or invalid name", async () => {
    render(<SaveSecretDialog location={location} secret="abc" defaultName="base" onSaved={vi.fn()} onClose={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("already exists");
    await userEvent.clear(screen.getByLabelText("Variable name"));
    await userEvent.type(screen.getByLabelText("Variable name"), "$x");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("reserved");
    expect(api.writeVariables).not.toHaveBeenCalled();
  });
});
