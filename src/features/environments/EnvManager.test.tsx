import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Variable } from "../../ipc/bindings";
import * as api from "./api";
import { EnvManager } from "./EnvManager";
import { useEnvStore } from "./envStore";

vi.mock("./api");
vi.mock("../collections/api");

const v = (over: Partial<Variable>): Variable => ({
  name: "a", value: "1", current: null, enabled: true, secret: false, has_secret: false, ...over,
});

beforeEach(() => {
  vi.resetAllMocks();
  useEnvStore.setState(useEnvStore.getInitialState(), true);
  useEnvStore.setState({
    byCollection: { c1: { environments: [{ env: "dev", name: "Dev", invalid: false }], selected: null } },
  });
  vi.mocked(api.readVariables).mockImplementation(async (scope) =>
    scope.scope === "environment" ? [v({ name: "token", value: "", secret: true, has_secret: true })] : [v({})],
  );
  vi.mocked(api.writeVariables).mockImplementation(async (_s, inputs) =>
    inputs.map((i) => v({ name: i.name, value: i.value })),
  );
});

const open = (collectionId: string | null = "c1") => render(<EnvManager collectionId={collectionId} onClose={vi.fn()} />);

describe("EnvManager", () => {
  it("edits and saves globals, then refetches effective variables", async () => {
    open();
    const name = await screen.findByLabelText("Variable name 1");
    await userEvent.clear(screen.getByLabelText("Variable value 1"));
    await userEvent.type(screen.getByLabelText("Variable value 1"), "9");
    expect(name).toHaveValue("a");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.writeVariables).toHaveBeenCalledWith(
      { scope: "global" },
      [expect.objectContaining({ name: "a", previous_name: "a", value: "9" })],
    );
  });

  it("blocks Save on invalid names", async () => {
    open();
    await userEvent.type(await screen.findByLabelText("Variable name 2"), "$bad");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("reserved");
    expect(api.writeVariables).not.toHaveBeenCalled();
  });

  it("hides collection scopes for scratch", async () => {
    open(null);
    await screen.findByLabelText("Variable name 1");
    expect(screen.queryByRole("button", { name: "Collection variables" })).toBeNull();
    expect(screen.queryByLabelText("Current value 1")).toBeNull();
  });

  it("shows a secret as write-only with a Stored placeholder and sends Set", async () => {
    open();
    await userEvent.click(screen.getByRole("button", { name: "Dev" }));
    const input = await screen.findByLabelText("Secret value 1");
    expect(input).toHaveValue("");
    expect(input).toHaveAttribute("placeholder", "Stored");
    await userEvent.type(input, "s3cret");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.writeVariables).toHaveBeenCalledWith(
      { scope: "environment", collection_id: "c1", env: "dev" },
      [expect.objectContaining({ secret: true, value: "", secret_edit: { op: "set", value: "s3cret" } })],
    );
  });

  it("sends Clear for a cleared secret", async () => {
    open();
    await userEvent.click(screen.getByRole("button", { name: "Dev" }));
    await userEvent.click(await screen.findByRole("button", { name: "Clear secret 1" }));
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(vi.mocked(api.writeVariables).mock.calls[0]?.[1][0]?.secret_edit).toEqual({ op: "clear" });
  });

  it("asks before dropping edits when switching scope", async () => {
    open();
    await userEvent.type(await screen.findByLabelText("Variable name 2"), "x");
    await userEvent.click(screen.getByRole("button", { name: "Dev" }));
    const confirm = await screen.findByRole("dialog", { name: "Discard changes?" });
    await userEvent.click(within(confirm).getByRole("button", { name: "Discard" }));
    expect(await screen.findByLabelText("Secret value 1")).toBeVisible();
  });

  it("warns that a plain value turned secret may be in Git history", async () => {
    open();
    await screen.findByLabelText("Variable name 1");
    await userEvent.click(screen.getByLabelText("Secret variable 1"));
    expect(screen.getByText("This value may already be in Git history. Rotate it.")).toBeVisible();
  });

  it("resets a current value", async () => {
    vi.mocked(api.readVariables).mockResolvedValue([v({ current: "local" })]);
    open();
    await userEvent.click(screen.getByRole("button", { name: "Collection variables" }));
    expect(await screen.findByLabelText("Current value 1")).toHaveValue("local");
    await userEvent.click(screen.getByRole("button", { name: "Reset current value 1" }));
    expect(screen.getByLabelText("Current value 1")).toHaveValue("");
  });

  it("creates an environment and selects it", async () => {
    const created = { environments: [{ env: "dev", name: "Dev", invalid: false }, { env: "qa", name: "QA", invalid: false }], selected: null };
    vi.mocked(api.createEnvironment).mockResolvedValue(created);
    vi.mocked(api.effectiveVariables).mockResolvedValue([]);
    open();
    await userEvent.click(screen.getByRole("button", { name: "New environment" }));
    await userEvent.type(screen.getByLabelText("Name"), "QA");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(api.createEnvironment).toHaveBeenCalledWith("c1", "QA");
    await vi.waitFor(() =>
      expect(api.readVariables).toHaveBeenLastCalledWith({ scope: "environment", collection_id: "c1", env: "qa" }),
    );
  });

  it("deletes after confirmation", async () => {
    vi.mocked(api.deleteEnvironment).mockResolvedValue({ environments: [], selected: null });
    open();
    await userEvent.click(screen.getByRole("button", { name: "Dev" }));
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    await userEvent.click(within(await screen.findByRole("dialog", { name: "Delete environment?" })).getByRole("button", { name: "Delete" }));
    expect(api.deleteEnvironment).toHaveBeenCalledWith("c1", "dev");
  });

  it("offers to reset damaged secret storage", async () => {
    useEnvStore.setState({ storage: { mode: "damaged", message: "Vault is damaged." } });
    vi.mocked(api.resetSecretStorage).mockResolvedValue({ mode: "keychain", message: null });
    open();
    expect(screen.getByText("Vault is damaged.")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Reset secret storage" }));
    await userEvent.click(within(await screen.findByRole("dialog", { name: "Reset secret storage?" })).getByRole("button", { name: "Reset" }));
    expect(api.resetSecretStorage).toHaveBeenCalled();
  });

  it("warns but offers no reset when secrets are session-only", () => {
    useEnvStore.setState({ storage: { mode: "session_only", message: "Session only." } });
    open();
    expect(screen.getByText("Session only.")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Reset secret storage" })).not.toBeInTheDocument();
  });
});
