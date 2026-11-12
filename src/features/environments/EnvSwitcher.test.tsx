import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useTabsStore, WELCOME_TAB } from "../../shell/tabsStore";
import * as api from "./api";
import { EnvSwitcher } from "./EnvSwitcher";
import { useEnvStore } from "./envStore";

vi.mock("./api");
vi.mock("../collections/api");

const list = {
  environments: [
    { env: "dev", name: "Dev", invalid: false },
    { env: "bad", name: "Broken", invalid: true },
  ],
  selected: "dev",
};

const showTab = (location: ReturnType<typeof useTabsStore.getState>["tabs"][number]["location"]) =>
  useTabsStore.setState({
    tabs: [WELCOME_TAB, { id: "t1", kind: "request", title: "R", location }],
    activeTabId: "t1",
  });

beforeEach(() => {
  vi.resetAllMocks();
  useEnvStore.setState(useEnvStore.getInitialState(), true);
  vi.mocked(api.listEnvironments).mockResolvedValue(list);
  vi.mocked(api.secretStorageStatus).mockResolvedValue({ mode: "keychain", message: null });
  vi.mocked(api.effectiveVariables).mockResolvedValue([]);
  vi.mocked(api.selectEnvironment).mockResolvedValue(null);
});

describe("EnvSwitcher", () => {
  it("lists valid environments and selects through the API", async () => {
    showTab({ scope: "collection", collection_id: "c1", path: "a.http.yaml" });
    render(<EnvSwitcher />);
    const select = await screen.findByLabelText("Environment");
    await vi.waitFor(() => expect(select).toHaveValue("dev"));
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["No environment", "Dev"]);

    await userEvent.selectOptions(select, "");
    expect(api.selectEnvironment).toHaveBeenCalledWith("c1", null);
  });

  it("is disabled for scratch tabs and explains why", () => {
    showTab({ scope: "scratch", id: "s1" });
    render(<EnvSwitcher />);
    const select = screen.getByLabelText("Environment");
    expect(select).toBeDisabled();
    expect(select).toHaveAttribute("title", "Scratch requests use global variables only");
  });

  it("shows a badge only when secrets are not in the keychain", async () => {
    showTab({ scope: "scratch", id: "s1" });
    vi.mocked(api.secretStorageStatus).mockResolvedValue({ mode: "session_only", message: "No keychain." });
    render(<EnvSwitcher />);
    expect(await screen.findByText("Secrets: session only")).toBeVisible();
  });

  it("shows no badge for the keychain", async () => {
    showTab({ scope: "scratch", id: "s1" });
    render(<EnvSwitcher />);
    await vi.waitFor(() => expect(api.secretStorageStatus).toHaveBeenCalled());
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("opens the variables manager", async () => {
    showTab({ scope: "scratch", id: "s1" });
    vi.mocked(api.readVariables).mockResolvedValue([]);
    render(<EnvSwitcher />);
    await userEvent.click(screen.getByRole("button", { name: "Variables…" }));
    expect(await screen.findByRole("dialog", { name: "Variables" })).toBeVisible();
  });
});
