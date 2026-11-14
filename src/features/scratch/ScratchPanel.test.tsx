import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useTabsStore } from "../../shell/tabsStore";
import { makeTree } from "../../test/fixtures";
import { useToastStore } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as api from "./api";
import { ScratchPanel } from "./ScratchPanel";
import { useScratchStore } from "./scratchStore";

vi.mock("./api");

const item = (id: string, name: string) => ({ id, name, method: "GET", kind: "http" as const });

async function pick(name: string, entry: string) {
  await userEvent.click(screen.getByRole("menuitem", { name: `Actions for ${name}` }));
  await userEvent.click(screen.getByRole("menuitem", { name: entry }));
}

describe("ScratchPanel", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useScratchStore.setState({ items: [] });
    useTabsStore.setState(useTabsStore.getInitialState(), true);
    useCollectionsStore.setState(useCollectionsStore.getInitialState(), true);
    useToastStore.setState({ toasts: [] });
  });

  it("shows the empty text and creates a request that opens a tab", async () => {
    vi.mocked(api.createScratch).mockResolvedValue(item("a", "Untitled"));
    render(<ScratchPanel />);
    expect(screen.getByText("No scratch requests yet.")).toBeVisible();

    await userEvent.click(screen.getByRole("menuitem", { name: "New Request" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "HTTP" }));

    expect(await screen.findByRole("button", { name: /Untitled/ })).toBeVisible();
    expect(useTabsStore.getState().tabs[1].title).toBe("Untitled");
  });

  it("offers GraphQL and SOAP and shows their badge", async () => {
    vi.mocked(api.createScratch).mockResolvedValue({ id: "g", name: "Q", method: "POST", kind: "graphql" });
    render(<ScratchPanel />);

    await userEvent.click(screen.getByRole("menuitem", { name: "New Request" }));
    expect(screen.getAllByRole("menuitem").map((e) => e.textContent)).toEqual(
      expect.arrayContaining(["HTTP", "GraphQL", "SOAP"]),
    );
    await userEvent.click(screen.getByRole("menuitem", { name: "GraphQL" }));

    expect(api.createScratch).toHaveBeenCalledWith("graphql");
    expect(await screen.findByRole("button", { name: /Q/ })).toHaveTextContent("GQL");
  });

  it("clicking a row opens its tab", async () => {
    useScratchStore.setState({ items: [item("a", "Alpha")] });
    render(<ScratchPanel />);
    await userEvent.click(screen.getByRole("button", { name: /Alpha/ }));
    expect(useTabsStore.getState().tabs[1].location).toEqual({ scope: "scratch", id: "a" });
  });

  it("renames through the dialog", async () => {
    useScratchStore.setState({ items: [item("a", "Alpha")] });
    vi.mocked(api.renameScratch).mockResolvedValue([item("a", "Beta")]);
    render(<ScratchPanel />);

    await pick("Alpha", "Rename");
    await userEvent.clear(screen.getByLabelText("Name"));
    await userEvent.type(screen.getByLabelText("Name"), "Beta{Enter}");

    expect(api.renameScratch).toHaveBeenCalledWith("a", "Beta");
    expect(await screen.findByRole("button", { name: /Beta/ })).toBeVisible();
  });

  it("deletes a row", async () => {
    useScratchStore.setState({ items: [item("a", "Alpha")] });
    vi.mocked(api.deleteScratch).mockResolvedValue([]);
    render(<ScratchPanel />);
    await pick("Alpha", "Delete");
    expect(await screen.findByText("No scratch requests yet.")).toBeVisible();
  });

  it("move dialog without collections explains and disables Move", async () => {
    useScratchStore.setState({ items: [item("a", "Alpha")] });
    render(<ScratchPanel />);
    await pick("Alpha", "Move to collection…");
    expect(screen.getByText("Open or create a collection first.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Move" })).toBeDisabled();
  });

  it("move dialog lists available collections only, with nested folders", async () => {
    useScratchStore.setState({ items: [item("a", "Alpha")] });
    useCollectionsStore.setState({ trees: [makeTree("c1", "One"), makeTree("c2", "Gone", false), makeTree("c3", "Three")] });
    render(<ScratchPanel />);
    await pick("Alpha", "Move to collection…");

    const collections = screen.getByLabelText("Collection");
    expect(Array.from(collections.querySelectorAll("option")).map((o) => o.textContent)).toEqual(["One", "Three"]);
    expect(Array.from(screen.getByLabelText("Folder").querySelectorAll("option")).map((o) => o.textContent)).toEqual(["(root)", "auth"]);
  });

  it("Move sends the chosen collection and folder, then closes and toasts", async () => {
    useScratchStore.setState({ items: [item("a", "Alpha")] });
    useCollectionsStore.setState({ trees: [makeTree("c1", "One"), makeTree("c3", "Three")] });
    vi.mocked(api.moveScratchToCollection).mockResolvedValue({ tree: makeTree("c3", "Three"), path: "auth/a.http.yaml" });
    render(<ScratchPanel />);
    await pick("Alpha", "Move to collection…");

    await userEvent.selectOptions(screen.getByLabelText("Collection"), "c3");
    await userEvent.selectOptions(screen.getByLabelText("Folder"), "auth");
    await userEvent.click(screen.getByRole("button", { name: "Move" }));

    expect(api.moveScratchToCollection).toHaveBeenCalledWith("a", "c3", "auth");
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByText("No scratch requests yet.")).toBeVisible();
    expect(useToastStore.getState().toasts[0].kind).toBe("info");
  });

  it("Move failure keeps the dialog open and shows an error toast", async () => {
    useScratchStore.setState({ items: [item("a", "Alpha")] });
    useCollectionsStore.setState({ trees: [makeTree("c1", "One")] });
    vi.mocked(api.moveScratchToCollection).mockRejectedValue({ code: "io", message: "Disk full" });
    render(<ScratchPanel />);
    await pick("Alpha", "Move to collection…");

    await userEvent.click(screen.getByRole("button", { name: "Move" }));

    await vi.waitFor(() => expect(useToastStore.getState().toasts[0]?.message).toBe("Disk full"));
    expect(screen.getByRole("dialog")).toBeVisible();
    expect(useScratchStore.getState().items).toHaveLength(1);
  });

  it("Cancel calls nothing", async () => {
    useScratchStore.setState({ items: [item("a", "Alpha")] });
    render(<ScratchPanel />);
    await pick("Alpha", "Move to collection…");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.moveScratchToCollection).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
