import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useTabsStore } from "../../shell/tabsStore";
import { makeTree } from "../../test/fixtures";
import { useToastStore } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as api from "./api";
import { JourneysPanel } from "./JourneysPanel";
import { useJourneyStore } from "./journeyStore";

vi.mock("./api");

const listed = [
  { file: "signup.journey.yaml", name: "Signup", nodes: 4, error: null },
];

describe("JourneysPanel", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useCollectionsStore.setState({ trees: [makeTree()] });
    useJourneyStore.setState({ byCollection: {}, runs: {} });
    useToastStore.setState({ toasts: [] });
    useTabsStore.setState({ tabs: [], activeTabId: null });
    vi.mocked(api.listJourneys).mockResolvedValue(listed);
  });

  it("says so without collections", () => {
    useCollectionsStore.setState({ trees: [] });
    render(<JourneysPanel />);
    expect(screen.getByText(/No journeys yet/)).toBeVisible();
  });

  it("lists journeys and opens one in a tab", async () => {
    render(<JourneysPanel />);
    await userEvent.click(
      await screen.findByRole("button", { name: /^Signup\s*4/ }),
    );
    expect(useTabsStore.getState().tabs[0]).toMatchObject({
      kind: "journey",
      journey: { collectionId: "c1", file: "signup.journey.yaml" },
    });
  });

  it("creates a journey with a Start node and opens it", async () => {
    vi.mocked(api.saveJourney).mockResolvedValue(listed[0]!);
    render(<JourneysPanel />);
    await userEvent.click(
      screen.getByRole("button", { name: "New journey in My API" }),
    );
    await vi.waitFor(() =>
      expect(useTabsStore.getState().tabs).toHaveLength(1),
    );
    const sent = vi.mocked(api.saveJourney).mock.calls[0]!;
    expect(sent[1]).toBeNull();
    expect(sent[2].nodes?.map((n) => n.type)).toEqual(["start"]);
  });

  it("deletes a journey and closes its tab", async () => {
    vi.mocked(api.deleteJourney).mockResolvedValue(null);
    useTabsStore.getState().openJourney("c1", "signup.journey.yaml", "Signup");
    render(<JourneysPanel />);
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "Actions for Signup" }),
    );
    await userEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    await vi.waitFor(() =>
      expect(useTabsStore.getState().tabs).toHaveLength(0),
    );
    expect(api.deleteJourney).toHaveBeenCalledWith("c1", "signup.journey.yaml");
  });
});
