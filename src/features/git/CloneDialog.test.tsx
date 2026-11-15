import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeTree } from "../../test/fixtures";
import { useToastStore } from "../../ui/toastStore";
import * as collectionsApi from "../collections/api";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as api from "./api";
import { CloneDialog } from "./CloneDialog";

vi.mock("./api");
vi.mock("../collections/api");

describe("CloneDialog", () => {
  const onClose = vi.fn();
  beforeEach(() => {
    vi.resetAllMocks();
    useCollectionsStore.setState(useCollectionsStore.getInitialState(), true);
    useToastStore.setState({ toasts: [] });
    vi.mocked(collectionsApi.listCollections).mockResolvedValue([
      makeTree("c9", "Cloned"),
    ]);
  });

  it("needs a URL before Clone is enabled", () => {
    render(<CloneDialog onClose={onClose} />);
    expect(screen.getByRole("button", { name: "Clone" })).toBeDisabled();
  });

  it("clones, reloads collections and closes", async () => {
    vi.mocked(api.clone).mockResolvedValue(makeTree("c9", "Cloned"));
    render(<CloneDialog onClose={onClose} />);
    await userEvent.type(
      screen.getByLabelText("Repository URL"),
      " https://x/y.git ",
    );
    await userEvent.click(screen.getByRole("button", { name: "Clone" }));
    expect(api.clone).toHaveBeenCalledWith("https://x/y.git");
    await vi.waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(useCollectionsStore.getState().trees[0]?.collection.id).toBe("c9");
  });

  it("stays open when the folder picker is cancelled", async () => {
    vi.mocked(api.clone).mockResolvedValue(null);
    render(<CloneDialog onClose={onClose} />);
    await userEvent.type(screen.getByLabelText("Repository URL"), "u");
    await userEvent.click(screen.getByRole("button", { name: "Clone" }));
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: "Clone" })).toBeEnabled(),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("reports a failed clone", async () => {
    vi.mocked(api.clone).mockRejectedValue({
      code: "invalid_input",
      message: "Nope.",
    });
    render(<CloneDialog onClose={onClose} />);
    await userEvent.type(screen.getByLabelText("Repository URL"), "u");
    await userEvent.click(screen.getByRole("button", { name: "Clone" }));
    await vi.waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    expect(onClose).not.toHaveBeenCalled();
  });
});
