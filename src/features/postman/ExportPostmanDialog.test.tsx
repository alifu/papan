import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeTree } from "../../test/fixtures";
import { useToastStore } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as envApi from "../environments/api";
import * as api from "./api";
import { ExportPostmanDialog } from "./ExportPostmanDialog";

vi.mock("./api");
vi.mock("../environments/api");

describe("ExportPostmanDialog", () => {
  const onClose = vi.fn();
  beforeEach(() => {
    vi.resetAllMocks();
    useCollectionsStore.setState(useCollectionsStore.getInitialState(), true);
    useCollectionsStore.setState({ trees: [makeTree(), makeTree("c2", "Shop")] });
    useToastStore.setState({ toasts: [] });
    vi.mocked(envApi.listEnvironments).mockResolvedValue({
      environments: [
        { env: "dev", name: "Dev" } as never,
        { env: "prod", name: "Prod" } as never,
      ],
      selected: null,
    });
  });

  it("prefills the collection and defaults secrets off with no warning", async () => {
    render(<ExportPostmanDialog collectionId="c2" onClose={onClose} />);
    expect(screen.getByLabelText("Collection")).toHaveValue("c2");
    expect(await screen.findByLabelText("Dev")).not.toBeChecked();
    expect(screen.getByLabelText("Include secret values")).not.toBeChecked();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("warns when secret values are included and sends the options", async () => {
    vi.mocked(api.exportCollection).mockResolvedValue({
      files: ["shop.postman_collection.json", "dev.postman_environment.json"],
      report: [{ level: "skipped", file: "shop", path: "ws", message: "WebSocket requests are not exported" }],
    });
    render(<ExportPostmanDialog collectionId="c2" onClose={onClose} />);
    await userEvent.click(await screen.findByLabelText("Dev"));
    await userEvent.click(screen.getByLabelText("Include globals"));
    await userEvent.click(screen.getByLabelText("Include secret values"));
    expect(screen.getByRole("alert")).toHaveTextContent(/plain text/);
    await userEvent.click(screen.getByRole("button", { name: "Export" }));

    expect(api.exportCollection).toHaveBeenCalledWith({
      collection_id: "c2",
      environments: ["dev"],
      include_globals: true,
      include_secret_values: true,
    });
    expect(await screen.findByText("shop.postman_collection.json")).toBeVisible();
    expect(screen.getByText("WebSocket requests are not exported")).toBeVisible();
  });

  it("stays put when the folder dialog is cancelled", async () => {
    vi.mocked(api.exportCollection).mockResolvedValue(null);
    render(<ExportPostmanDialog collectionId={null} onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Export" }));
    expect(api.exportCollection).toHaveBeenCalledWith(expect.objectContaining({ collection_id: "c1" }));
    expect(screen.getByRole("button", { name: "Export" })).toBeEnabled();
    expect(useToastStore.getState().toasts).toEqual([]);
  });

  it("reloads environments when the collection changes and clears the choices", async () => {
    render(<ExportPostmanDialog collectionId="c1" onClose={onClose} />);
    await userEvent.click(await screen.findByLabelText("Dev"));
    await userEvent.selectOptions(screen.getByLabelText("Collection"), "c2");
    expect(envApi.listEnvironments).toHaveBeenLastCalledWith("c2");
    expect(await screen.findByLabelText("Dev")).not.toBeChecked();
  });

  it("reports a failure as a toast", async () => {
    vi.mocked(api.exportCollection).mockRejectedValue({ code: "io", message: "Disk full." });
    render(<ExportPostmanDialog collectionId="c1" onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Export" }));
    await vi.waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
  });
});
