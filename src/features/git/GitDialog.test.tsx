import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GitStatus } from "../../ipc/bindings";
import { useToastStore } from "../../ui/toastStore";
import * as collectionsApi from "../collections/api";
import * as api from "./api";
import { GitDialog } from "./GitDialog";

vi.mock("./api");
vi.mock("../collections/api");

const repo = (over: Partial<GitStatus> = {}): GitStatus => ({
  git_available: true,
  is_repo: true,
  branch: "main",
  upstream: "origin/main",
  ahead: 0,
  behind: 0,
  merging: false,
  files: [],
  ...over,
});

describe("GitDialog", () => {
  const onClose = vi.fn();
  beforeEach(() => {
    vi.resetAllMocks();
    useToastStore.setState({ toasts: [] });
    vi.mocked(api.branches).mockResolvedValue([
      { name: "main", current: true },
    ]);
    vi.mocked(collectionsApi.readTree).mockRejectedValue(new Error("unused"));
  });

  it("offers Git installation advice when git is missing", async () => {
    vi.mocked(api.status).mockResolvedValue({
      ...repo(),
      git_available: false,
      is_repo: false,
    });
    render(<GitDialog collectionId="c1" onClose={onClose} />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Git could not be started",
    );
  });

  it("initialises a folder that is not a repo", async () => {
    vi.mocked(api.status).mockResolvedValueOnce({ ...repo(), is_repo: false });
    vi.mocked(api.init).mockResolvedValue(repo());
    vi.mocked(api.status).mockResolvedValue(repo());
    render(<GitDialog collectionId="c1" onClose={onClose} />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Initialise repository" }),
    );
    expect(api.init).toHaveBeenCalledWith("c1");
    expect(
      await screen.findByRole("button", { name: "Pull" }),
    ).toBeInTheDocument();
  });

  it("shows changes, a diff, and commits with a message", async () => {
    vi.mocked(api.status).mockResolvedValue(
      repo({
        ahead: 1,
        files: [
          {
            path: "requests/a.http.yaml",
            index: ".",
            worktree: "M",
            conflicted: false,
          },
        ],
      }),
    );
    vi.mocked(api.diff).mockResolvedValue("+new\n-old");
    vi.mocked(api.commit).mockResolvedValue({ message: "Committed" });
    render(<GitDialog collectionId="c1" onClose={onClose} />);
    expect(await screen.findByText("Modified")).toBeInTheDocument();
    expect(screen.getByLabelText("Sync status")).toHaveTextContent("↑1");
    expect(screen.getByRole("button", { name: "Commit" })).toBeDisabled();

    await userEvent.click(
      screen.getByRole("button", { name: "requests/a.http.yaml" }),
    );
    expect(
      await screen.findByLabelText("Diff of requests/a.http.yaml"),
    ).toHaveTextContent("+new");

    await userEvent.type(screen.getByLabelText("Commit message"), "tweak");
    await userEvent.click(screen.getByRole("button", { name: "Commit" }));
    expect(api.commit).toHaveBeenCalledWith("c1", "tweak");
  });

  it("lists conflicts, resolves them and blocks committing", async () => {
    vi.mocked(api.status).mockResolvedValue(
      repo({
        merging: true,
        files: [
          { path: "a.yaml", index: "U", worktree: "U", conflicted: true },
        ],
      }),
    );
    vi.mocked(api.resolve).mockResolvedValue(repo());
    render(<GitDialog collectionId="c1" onClose={onClose} />);
    await userEvent.type(
      await screen.findByLabelText("Commit message"),
      "merge",
    );
    expect(screen.getByRole("button", { name: "Commit" })).toBeDisabled();
    await userEvent.click(
      screen.getByRole("button", { name: "Take theirs a.yaml" }),
    );
    expect(api.resolve).toHaveBeenCalledWith("c1", "a.yaml", "theirs");
  });

  it("pulls, pushes and switches branches", async () => {
    vi.mocked(api.status).mockResolvedValue(repo());
    vi.mocked(api.branches).mockResolvedValue([
      { name: "main", current: true },
      { name: "dev", current: false },
    ]);
    vi.mocked(api.pull).mockResolvedValue({ message: "Pulled." });
    vi.mocked(api.push).mockResolvedValue({ message: "Pushed." });
    vi.mocked(api.switchBranch).mockResolvedValue(repo({ branch: "dev" }));
    render(<GitDialog collectionId="c1" onClose={onClose} />);
    await userEvent.click(await screen.findByRole("button", { name: "Pull" }));
    expect(api.pull).toHaveBeenCalledWith("c1");
    await userEvent.click(screen.getByRole("button", { name: "Push" }));
    expect(api.push).toHaveBeenCalledWith("c1");
    await userEvent.selectOptions(screen.getByLabelText("Branch"), "dev");
    expect(api.switchBranch).toHaveBeenCalledWith("c1", "dev");
  });

  it("reports a failed action", async () => {
    vi.mocked(api.status).mockResolvedValue(repo());
    vi.mocked(api.push).mockRejectedValue({
      code: "invalid_input",
      message: "rejected",
    });
    render(<GitDialog collectionId="c1" onClose={onClose} />);
    await userEvent.click(await screen.findByRole("button", { name: "Push" }));
    await vi.waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
  });
});
