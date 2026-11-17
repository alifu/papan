import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useDialogsStore } from "../../shell/dialogsStore";
import { useTabsStore } from "../../shell/tabsStore";
import { makeTree } from "../../test/fixtures";
import { useToastStore } from "../../ui/toastStore";
import * as api from "./api";
import { CollectionsPanel } from "./CollectionsPanel";
import { expandKey, useCollectionsStore } from "./collectionsStore";

vi.mock("./api");

const rowNames = () =>
  screen.queryAllByRole("treeitem").map((r) => r.getAttribute("aria-label"));
const row = (name: string) => screen.getByRole("treeitem", { name });

async function pick(rowName: string, item: string) {
  await userEvent.click(
    screen.getByRole("menuitem", { name: `Actions for ${rowName}` }),
  );
  await userEvent.click(screen.getByRole("menuitem", { name: item }));
}

function seed(expandAuth = true) {
  const keys = [
    expandKey("c1", ""),
    ...(expandAuth ? [expandKey("c1", "auth")] : []),
  ];
  useCollectionsStore.setState({
    trees: [makeTree()],
    expanded: new Set(keys),
  });
}

describe("CollectionsPanel", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useCollectionsStore.setState(useCollectionsStore.getInitialState(), true);
    useTabsStore.setState(useTabsStore.getInitialState(), true);
    useToastStore.setState({ toasts: [] });
  });

  it("shows the empty text without collections", () => {
    render(<CollectionsPanel />);
    expect(screen.getByText("No collections yet.")).toBeVisible();
  });

  it("exposes tree roles, levels and expansion", () => {
    seed();
    render(<CollectionsPanel />);

    expect(screen.getByRole("tree")).toBeVisible();
    expect(rowNames()).toEqual([
      "My API",
      "health",
      "auth",
      "login",
      "logout",
      "broken",
    ]);
    expect(row("My API")).toHaveAttribute("aria-level", "1");
    expect(row("auth")).toHaveAttribute("aria-level", "2");
    expect(row("auth")).toHaveAttribute("aria-expanded", "true");
    expect(row("login")).toHaveAttribute("aria-level", "3");
    expect(row("health")).not.toHaveAttribute("aria-expanded");
  });

  it("clicking a folder toggles it and clicking a request opens its tab", async () => {
    seed(false);
    render(<CollectionsPanel />);

    await userEvent.click(row("auth"));
    expect(rowNames()).toContain("login");
    await userEvent.click(row("login"));

    expect(useTabsStore.getState().tabs[1]).toMatchObject({
      title: "login",
      location: {
        scope: "collection",
        collection_id: "c1",
        path: "auth/login.http.yaml",
      },
    });
  });

  it("navigates with the keyboard", async () => {
    seed(false);
    render(<CollectionsPanel />);
    row("My API").focus();

    await userEvent.keyboard("{ArrowDown}{ArrowDown}");
    expect(row("auth")).toHaveFocus();
    await userEvent.keyboard("{ArrowRight}");
    expect(row("auth")).toHaveAttribute("aria-expanded", "true");
    await userEvent.keyboard("{ArrowRight}");
    expect(row("login")).toHaveFocus();
    await userEvent.keyboard("{ArrowLeft}");
    expect(row("auth")).toHaveFocus();
    await userEvent.keyboard("{ArrowLeft}");
    expect(row("auth")).toHaveAttribute("aria-expanded", "false");
    await userEvent.keyboard("{ArrowLeft}");
    expect(row("My API")).toHaveFocus();
    await userEvent.keyboard("{End}");
    expect(row("broken")).toHaveFocus();
    await userEvent.keyboard("{Home}{ArrowUp}");
    expect(row("My API")).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}{Enter}");
    expect(useTabsStore.getState().tabs).toHaveLength(2);
  });

  it("offers menu items per node kind", async () => {
    seed();
    render(<CollectionsPanel />);
    const labels = async (name: string) => {
      await userEvent.click(
        screen.getByRole("menuitem", { name: `Actions for ${name}` }),
      );
      const items = within(screen.getByRole("menu"))
        .getAllByRole("menuitem")
        .map((i) => i.textContent);
      await userEvent.keyboard("{Escape}");
      return items;
    };
    expect(await labels("My API")).toEqual([
      "New Request",
      "New Folder",
      "Scripts…",
      "Export to Postman…",
      "Git…",
      "Close",
    ]);
    expect(await labels("auth")).toEqual([
      "New Request",
      "New Folder",
      "Scripts…",
      "Rename",
      "Move Up",
      "Move Down",
      "Delete",
    ]);
    expect(await labels("login")).toEqual([
      "Rename",
      "Move Up",
      "Move Down",
      "Delete",
    ]);
    expect(await labels("broken")).toEqual(["Delete"]);
  });

  it("Export to Postman opens the export dialog for that collection", async () => {
    seed();
    render(<CollectionsPanel />);
    await pick("My API", "Export to Postman…");
    expect(useDialogsStore.getState()).toMatchObject({
      open: "export-postman",
      collectionId: "c1",
    });
  });

  it("Git opens the git dialog for that collection", async () => {
    seed();
    render(<CollectionsPanel />);
    await pick("My API", "Git…");
    expect(useDialogsStore.getState()).toMatchObject({
      open: "git",
      collectionId: "c1",
    });
  });

  it("offers Trust scripts only for an untrusted collection and trusts it after confirming", async () => {
    const untrusted = makeTree();
    useCollectionsStore.setState({
      trees: [
        {
          ...untrusted,
          collection: { ...untrusted.collection, scripts_trusted: false },
        },
      ],
      expanded: new Set([expandKey("c1", "")]),
    });
    vi.mocked(api.setScriptsTrusted).mockResolvedValue(null);
    vi.mocked(api.readTree).mockResolvedValue(makeTree());
    render(<CollectionsPanel />);
    await pick("My API", "Trust scripts…");
    await userEvent.click(screen.getByRole("button", { name: "Trust" }));
    expect(api.setScriptsTrusted).toHaveBeenCalledWith("c1", true);
  });

  it("hides Trust scripts for a trusted collection", async () => {
    seed();
    render(<CollectionsPanel />);
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Actions for My API" }),
    );
    expect(
      screen.queryByRole("menuitem", { name: "Trust scripts…" }),
    ).toBeNull();
  });

  it("disables Move Up at the top and Move Down at the bottom", async () => {
    seed();
    render(<CollectionsPanel />);
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Actions for health" }),
    );
    expect(screen.getByRole("menuitem", { name: "Move Up" })).toBeDisabled();
    expect(screen.getByRole("menuitem", { name: "Move Down" })).toBeEnabled();
  });

  it("creates a request through the name dialog and opens its tab", async () => {
    seed();
    vi.mocked(api.createRequest).mockResolvedValue({
      tree: makeTree(),
      path: "auth/new.http.yaml",
    });
    render(<CollectionsPanel />);

    await pick("auth", "New Request");
    expect(screen.getByRole("dialog", { name: "New Request" })).toBeVisible();
    await userEvent.type(screen.getByLabelText("Name"), "New{Enter}");

    expect(api.createRequest).toHaveBeenCalledWith("c1", "auth", "New", "http");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(useTabsStore.getState().tabs[1].location).toMatchObject({
      path: "auth/new.http.yaml",
    });
  });

  it("creates a SOAP request when the Type choice is changed", async () => {
    seed();
    vi.mocked(api.createRequest).mockResolvedValue({
      tree: makeTree(),
      path: "auth/s.http.yaml",
    });
    render(<CollectionsPanel />);

    await pick("auth", "New Request");
    await userEvent.selectOptions(screen.getByLabelText("Type"), "soap");
    await userEvent.type(screen.getByLabelText("Name"), "s{Enter}");

    expect(api.createRequest).toHaveBeenCalledWith("c1", "auth", "s", "soap");
  });

  it("creates a WebSocket request when the Type choice is changed", async () => {
    seed();
    vi.mocked(api.createRequest).mockResolvedValue({
      tree: makeTree(),
      path: "auth/w.http.yaml",
    });
    render(<CollectionsPanel />);

    await pick("auth", "New Request");
    await userEvent.selectOptions(screen.getByLabelText("Type"), "ws");
    await userEvent.type(screen.getByLabelText("Name"), "w{Enter}");

    expect(api.createRequest).toHaveBeenCalledWith("c1", "auth", "w", "ws");
  });

  it("creates a gRPC request when the Type choice is changed", async () => {
    seed();
    vi.mocked(api.createRequest).mockResolvedValue({
      tree: makeTree(),
      path: "auth/g.http.yaml",
    });
    render(<CollectionsPanel />);

    await pick("auth", "New Request");
    await userEvent.selectOptions(screen.getByLabelText("Type"), "grpc");
    await userEvent.type(screen.getByLabelText("Name"), "g{Enter}");

    expect(api.createRequest).toHaveBeenCalledWith("c1", "auth", "g", "grpc");
  });

  it("shows the protocol badge for GraphQL and SOAP rows", () => {
    const tree = makeTree();
    const retag = (path: string, request_kind: "graphql" | "soap") =>
      tree.children.map((n) => (n.path === path ? { ...n, request_kind } : n));
    useCollectionsStore.setState({
      trees: [{ ...tree, children: retag("health.http.yaml", "graphql") }],
      expanded: new Set(["c1:"]),
    });
    render(<CollectionsPanel />);
    const row = screen.getByRole("treeitem", { name: "health" });
    expect(row).toHaveTextContent("GQL");
  });

  it("creates a folder and a collection", async () => {
    seed();
    vi.mocked(api.createFolder).mockResolvedValue({
      tree: makeTree(),
      path: "x",
    });
    vi.mocked(api.createCollection).mockResolvedValue(makeTree("c2", "Second"));
    render(<CollectionsPanel />);

    await pick("My API", "New Folder");
    await userEvent.type(screen.getByLabelText("Name"), "Docs{Enter}");
    expect(api.createFolder).toHaveBeenCalledWith("c1", "", "Docs");

    await userEvent.click(
      screen.getByRole("button", { name: "New Collection" }),
    );
    await userEvent.type(screen.getByLabelText("Name"), "Second{Enter}");
    expect(api.createCollection).toHaveBeenCalledWith("Second");
    expect(
      await screen.findByRole("treeitem", { name: "Second" }),
    ).toBeVisible();
  });

  it("opens a collection from the picker", async () => {
    vi.mocked(api.openCollection).mockResolvedValue(makeTree("c3", "Picked"));
    render(<CollectionsPanel />);
    await userEvent.click(
      screen.getByRole("button", { name: "Open Collection" }),
    );
    expect(
      await screen.findByRole("treeitem", { name: "Picked" }),
    ).toBeVisible();
  });

  it("renames a request and retargets its open tab", async () => {
    seed();
    useTabsStore
      .getState()
      .openRequest(
        { scope: "collection", collection_id: "c1", path: "health.http.yaml" },
        "health",
      );
    vi.mocked(api.renameNode).mockResolvedValue({
      tree: makeTree(),
      path: "ping.http.yaml",
    });
    render(<CollectionsPanel />);

    await pick("health", "Rename");
    await userEvent.clear(screen.getByLabelText("Name"));
    await userEvent.type(screen.getByLabelText("Name"), "ping{Enter}");

    expect(api.renameNode).toHaveBeenCalledWith(
      "c1",
      "health.http.yaml",
      "ping",
    );
    expect(useTabsStore.getState().tabs[1]).toMatchObject({
      title: "ping",
      location: { path: "ping.http.yaml" },
    });
  });

  it("deletes after confirmation and closes the tab; the folder wording warns", async () => {
    seed();
    useTabsStore
      .getState()
      .openRequest(
        {
          scope: "collection",
          collection_id: "c1",
          path: "auth/login.http.yaml",
        },
        "login",
      );
    vi.mocked(api.deleteNode).mockResolvedValue({
      tree: makeTree(),
      path: null,
    });
    render(<CollectionsPanel />);

    await pick("auth", "Delete");
    expect(screen.getByText(/and everything in it/)).toBeVisible();
    expect(api.deleteNode).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));

    expect(api.deleteNode).toHaveBeenCalledWith("c1", "auth");
    await vi.waitFor(() =>
      expect(useTabsStore.getState().tabs).toHaveLength(1),
    );
  });

  it("cancelling the confirm dialog deletes nothing", async () => {
    seed();
    render(<CollectionsPanel />);
    await pick("health", "Delete");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.deleteNode).not.toHaveBeenCalled();
  });

  it("Move Down moves one place and Move Up goes back", async () => {
    seed();
    vi.mocked(api.moveNode).mockResolvedValue({
      tree: makeTree(),
      path: "moved",
    });
    render(<CollectionsPanel />);

    await pick("health", "Move Down");
    expect(api.moveNode).toHaveBeenLastCalledWith(
      "c1",
      "health.http.yaml",
      "",
      1,
    );
    await pick("auth", "Move Up");
    expect(api.moveNode).toHaveBeenLastCalledWith("c1", "auth", "", 0);
  });

  it("Close forgets the collection and closes its tabs", async () => {
    seed();
    useTabsStore
      .getState()
      .openRequest(
        { scope: "collection", collection_id: "c1", path: "health.http.yaml" },
        "health",
      );
    vi.mocked(api.closeCollection).mockResolvedValue(null);
    render(<CollectionsPanel />);

    await pick("My API", "Close");

    await vi.waitFor(() =>
      expect(useTabsStore.getState().tabs).toHaveLength(1),
    );
    expect(screen.getByText("No collections yet.")).toBeVisible();
  });

  it("marks an unavailable collection as missing with only Close", async () => {
    useCollectionsStore.setState({
      trees: [{ ...makeTree("c1", "Gone", false), children: [] }],
    });
    render(<CollectionsPanel />);
    expect(
      screen.getByRole("treeitem", { name: "Gone (missing)" }),
    ).not.toHaveAttribute("aria-expanded");
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Actions for Gone" }),
    );
    expect(
      within(screen.getByRole("menu")).getAllByRole("menuitem"),
    ).toHaveLength(1);
  });

  describe("drag and drop", () => {
    beforeEach(() => {
      seed();
      vi.mocked(api.moveNode).mockResolvedValue({
        tree: makeTree(),
        path: "moved",
      });
      render(<CollectionsPanel />);
    });

    it("dropping on a request inserts before it", () => {
      fireEvent.dragStart(row("logout"));
      fireEvent.drop(row("health"));
      expect(api.moveNode).toHaveBeenCalledWith(
        "c1",
        "auth/logout.http.yaml",
        "",
        0,
      );
    });

    it("dropping later in the same folder accounts for the removed item", () => {
      fireEvent.dragStart(row("login"));
      fireEvent.drop(row("logout"));
      expect(api.moveNode).toHaveBeenCalledWith(
        "c1",
        "auth/login.http.yaml",
        "auth",
        0,
      );
    });

    it("dropping on a folder appends into it", () => {
      fireEvent.dragStart(row("health"));
      fireEvent.drop(row("auth"));
      expect(api.moveNode).toHaveBeenCalledWith(
        "c1",
        "health.http.yaml",
        "auth",
        2,
      );
    });

    it("ignores drops on itself, its descendants, the collection row and after dragend", () => {
      fireEvent.dragStart(row("auth"));
      fireEvent.drop(row("auth"));
      fireEvent.drop(row("login"));
      fireEvent.drop(row("My API"));
      fireEvent.dragStart(row("health"));
      fireEvent.dragEnd(row("health"));
      fireEvent.drop(row("auth"));
      expect(api.moveNode).not.toHaveBeenCalled();
    });

    it("ignores a drop with nothing dragged and refuses to drag the collection", () => {
      fireEvent.drop(row("health"));
      fireEvent.dragStart(row("My API"));
      fireEvent.drop(row("health"));
      expect(api.moveNode).not.toHaveBeenCalled();
    });

    it("ignores drops across collections", () => {
      act(() => {
        useCollectionsStore.setState({
          trees: [makeTree("c1"), makeTree("c2", "Other")],
        });
        useCollectionsStore.getState().expand(expandKey("c2", ""));
      });
      const [mine, theirs] = screen.getAllByRole("treeitem", {
        name: "health",
      });
      fireEvent.dragStart(mine);
      fireEvent.dragOver(theirs);
      fireEvent.drop(theirs);
      expect(api.moveNode).not.toHaveBeenCalled();
    });
  });
});
