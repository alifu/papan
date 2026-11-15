import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useTabsStore } from "../../shell/tabsStore";
import { useToastStore } from "../../ui/toastStore";
import { makeTree } from "../../test/fixtures";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as api from "./api";
import { MocksPanel } from "./MocksPanel";
import { useMockStore } from "./mockStore";

vi.mock("./api");

const listed = (running = false) => [
  {
    file: "users.mock.yaml",
    name: "Users",
    port: 0,
    routes: 2,
    running,
    url: running ? "http://127.0.0.1:1" : null,
    error: null,
  },
];

describe("MocksPanel", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useCollectionsStore.setState({ trees: [makeTree()] });
    useMockStore.setState({ byCollection: {}, logs: {} });
    useToastStore.setState({ toasts: [] });
    useTabsStore.setState({ tabs: [], activeTabId: null });
    vi.mocked(api.listMocks).mockResolvedValue(listed());
  });

  it("says so without collections", () => {
    useCollectionsStore.setState({ trees: [] });
    render(<MocksPanel />);
    expect(screen.getByText(/No mock servers yet/)).toBeVisible();
  });

  it("lists mocks per collection and opens one in a tab", async () => {
    render(<MocksPanel />);
    await userEvent.click(
      await screen.findByRole("button", { name: /^Users\s*\d/ }),
    );
    expect(useTabsStore.getState().tabs[0]).toMatchObject({
      kind: "mock",
      mock: { collectionId: "c1", file: "users.mock.yaml" },
    });
  });

  it("creates a mock and opens it", async () => {
    vi.mocked(api.saveMock).mockResolvedValue(listed()[0]!);
    render(<MocksPanel />);
    await userEvent.click(
      screen.getByRole("button", { name: "New mock in My API" }),
    );
    await vi.waitFor(() =>
      expect(useTabsStore.getState().tabs).toHaveLength(1),
    );
    expect(api.saveMock).toHaveBeenCalledWith(
      "c1",
      null,
      expect.objectContaining({ name: "New mock" }),
    );
  });

  it("starts and deletes", async () => {
    vi.mocked(api.startMock).mockResolvedValue("http://127.0.0.1:1");
    vi.mocked(api.deleteMock).mockResolvedValue(null);
    render(<MocksPanel />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Start Users" }),
    );
    expect(api.startMock).toHaveBeenCalledWith("c1", "users.mock.yaml");
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Actions for Users" }),
    );
    await userEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(api.deleteMock).toHaveBeenCalledWith("c1", "users.mock.yaml");
  });

  it("creates a mock from the collection's requests and opens it", async () => {
    vi.mocked(api.mockFromCollection).mockResolvedValue({
      ...listed()[0]!,
      file: "my-api-mock.mock.yaml",
      name: "My API mock",
    });
    render(<MocksPanel />);
    await userEvent.click(
      screen.getByRole("button", { name: "Mock from the requests of My API" }),
    );
    await vi.waitFor(() =>
      expect(useTabsStore.getState().tabs).toHaveLength(1),
    );
    expect(api.mockFromCollection).toHaveBeenCalledWith("c1");
    expect(useTabsStore.getState().tabs[0]).toMatchObject({
      kind: "mock",
      mock: { file: "my-api-mock.mock.yaml" },
    });
  });

  it("reports why a mock could not be made", async () => {
    vi.mocked(api.mockFromCollection).mockRejectedValue({
      code: "invalid_input",
      message: "No requests to mock.",
    });
    render(<MocksPanel />);
    await userEvent.click(
      screen.getByRole("button", { name: "Mock from the requests of My API" }),
    );
    await vi.waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    expect(useTabsStore.getState().tabs).toHaveLength(0);
  });
  it("imports a Mockers file, opens it and reports what was left out", async () => {
    vi.mocked(api.importMock).mockResolvedValue({
      summary: { ...listed()[0]!, file: "shop.mock.yaml", name: "Shop" },
      notes: ["A rule was left out."],
    });
    render(<MocksPanel />);
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Import a mock into My API" }),
    );
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Mockers file…" }),
    );
    await vi.waitFor(() =>
      expect(useTabsStore.getState().tabs).toHaveLength(1),
    );
    expect(api.importMock).toHaveBeenCalledWith("c1", "mockers");
    expect(useToastStore.getState().toasts[0]?.message).toMatch(
      /A rule was left out/,
    );
  });

  it("does nothing when the file dialog is cancelled", async () => {
    vi.mocked(api.importMock).mockResolvedValue(null);
    render(<MocksPanel />);
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Import a mock into My API" }),
    );
    await userEvent.click(
      screen.getByRole("menuitem", { name: "OpenAPI file…" }),
    );
    await vi.waitFor(() => expect(api.importMock).toHaveBeenCalled());
    expect(api.importMock).toHaveBeenCalledWith("c1", "openapi");
    expect(useTabsStore.getState().tabs).toHaveLength(0);
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("exports a mock as a Mockers file", async () => {
    vi.mocked(api.exportMock).mockResolvedValue("users.mockers.json");
    render(<MocksPanel />);
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "Actions for Users" }),
    );
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Export as Mockers…" }),
    );
    await vi.waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    expect(api.exportMock).toHaveBeenCalledWith("c1", "users.mock.yaml");
  });
});
