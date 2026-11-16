import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Journey } from "../../ipc/bindings";
import { useTabsStore } from "../../shell/tabsStore";
import { makeTree } from "../../test/fixtures";
import { useToastStore } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as api from "./api";
import { JourneyTab } from "./JourneyTab";
import { useJourneyStore } from "./journeyStore";
import { addNode, connect, newJourney, patchNode } from "./journeyModel";

vi.mock("./api");
// React Flow needs real layout; the canvas is covered by flowMap and journeyModel tests.
vi.mock("./JourneyCanvas", () => ({
  JourneyCanvas: (p: {
    journey: Journey;
    onSelect: (id: string | null) => void;
    onSelectEdge: (id: string) => void;
  }) => (
    <div>
      <span>{`${p.journey.nodes?.length} nodes`}</span>
      <button onClick={() => p.onSelect("n2")}>pick n2</button>
      <button onClick={() => p.onSelectEdge("e1")}>pick e1</button>
    </div>
  ),
}));

const ready = (): Journey => {
  let j = addNode(newJourney("Signup"), "request", { x: 0, y: 0 });
  j = connect(j, "n1", "n2");
  return patchNode(j, "n2", { request: "a.http.yaml" });
};

const show = () =>
  render(
    <JourneyTab collectionId="c1" file="signup.journey.yaml" tabId="t1" />,
  );

describe("JourneyTab", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useToastStore.setState({ toasts: [] });
    useJourneyStore.setState({ byCollection: {}, runs: {} });
    useCollectionsStore.setState({ trees: [makeTree()] });
    useTabsStore.setState({
      tabs: [{ id: "t1", kind: "journey", title: "Signup" }],
      activeTabId: "t1",
    });
    vi.mocked(api.readJourney).mockResolvedValue(ready());
    vi.mocked(api.listJourneys).mockResolvedValue([]);
  });

  it("loads the journey; Save waits for a change", async () => {
    show();
    expect(await screen.findByText("2 nodes")).toBeVisible();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("adds a node from the palette and saves the whole journey", async () => {
    vi.mocked(api.saveJourney).mockResolvedValue({
      file: "signup.journey.yaml",
      name: "Signup",
      nodes: 3,
      error: null,
    });
    show();
    await screen.findByText("2 nodes");
    await userEvent.click(
      screen.getByRole("button", { name: "Add Delay node" }),
    );
    expect(screen.getByText("3 nodes")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    const [, file, sent] = vi.mocked(api.saveJourney).mock.calls[0]!;
    expect(file).toBe("signup.journey.yaml");
    expect(sent.nodes).toHaveLength(3);
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("warns about a Request node with no request", async () => {
    vi.mocked(api.readJourney).mockResolvedValue(
      patchNode(ready(), "n2", { request: "" }),
    );
    show();
    expect(await screen.findByText(/no request chosen/i)).toBeVisible();
  });

  it("edits the selected node in the inspector", async () => {
    show();
    await screen.findByText("2 nodes");
    expect(screen.getByText("Select a node to edit it.")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "pick n2" }));
    await userEvent.type(screen.getByLabelText("Label"), "Login");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("runs the journey, saving first when it has changes", async () => {
    vi.mocked(api.saveJourney).mockResolvedValue({
      file: "signup.journey.yaml",
      name: "Signup",
      nodes: 3,
      error: null,
    });
    vi.mocked(api.runJourney).mockResolvedValue({
      run_id: "r",
      status: "passed",
      steps: 2,
      failures: 0,
      message: "",
    });
    show();
    await screen.findByText("2 nodes");
    await userEvent.click(screen.getByRole("button", { name: "Add End node" }));
    await userEvent.click(screen.getByRole("button", { name: "Run" }));
    expect(api.saveJourney).toHaveBeenCalledTimes(1);
    expect(api.runJourney).toHaveBeenCalledWith("c1", "signup.journey.yaml");
  });

  it("stops a running journey", async () => {
    vi.mocked(api.cancelJourney).mockResolvedValue(null);
    useJourneyStore.setState({
      runs: {
        "c1/signup.journey.yaml": {
          ...(await import("./runState")).EMPTY_RUN,
          running: true,
        },
      },
    });
    show();
    await userEvent.click(await screen.findByRole("button", { name: "Stop" }));
    expect(api.cancelJourney).toHaveBeenCalledWith("c1", "signup.journey.yaml");
  });

  it("edits what an edge carries and saves the mapping", async () => {
    vi.mocked(api.saveJourney).mockResolvedValue({
      file: "signup.journey.yaml",
      name: "Signup",
      nodes: 2,
      error: null,
    });
    show();
    await screen.findByText("2 nodes");
    await userEvent.click(screen.getByRole("button", { name: "pick e1" }));
    expect(
      screen.getByRole("heading", { name: "Start → Request" }),
    ).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Add mapping" }));
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    const sent = vi.mocked(api.saveJourney).mock.calls[0]![2];
    expect(sent.edges![0]!.map).toEqual([
      { name: "", source: "body", path: "" },
    ]);
  });

  it("tidies the layout", async () => {
    show();
    await screen.findByText("2 nodes");
    await userEvent.click(screen.getByRole("button", { name: "Tidy up" }));
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });
});
