import { beforeEach, describe, expect, it, vi } from "vitest";
import type { JourneyRunEvent, RunSummary } from "../../ipc/bindings";
import * as api from "./api";
import { journeyKey, useJourneyStore } from "./journeyStore";

vi.mock("./api");

const ev = (event: Partial<JourneyRunEvent["event"]>): JourneyRunEvent => ({
  run_id: "r1",
  collection_id: "c1",
  file: "f.journey.yaml",
  event: {
    kind: "node_started",
    node_id: "n1",
    status: null,
    message: "",
    response: null,
    vars: [],
    console: [],
    steps: 0,
    run_status: null,
    ...event,
  },
});

const KEY = journeyKey("c1", "f.journey.yaml");

describe("journeyStore", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useJourneyStore.setState({ byCollection: {}, runs: {} });
  });

  it("folds run events into the state of that journey only", () => {
    const store = useJourneyStore.getState();
    store.handleEvent(ev({}));
    store.handleEvent(
      ev({ kind: "node_finished", status: "passed", steps: 1 }),
    );
    expect(useJourneyStore.getState().runs[KEY]?.status.n1).toBe("passed");
    expect(Object.keys(useJourneyStore.getState().runs)).toEqual([KEY]);
  });

  it("clears the last run when a new one starts and closes it from the summary", async () => {
    useJourneyStore
      .getState()
      .handleEvent(ev({ kind: "node_finished", status: "failed" }));
    const summary: RunSummary = {
      run_id: "r2",
      status: "passed",
      steps: 0,
      failures: 0,
      message: "",
    };
    vi.mocked(api.runJourney).mockImplementation(async () => {
      expect(useJourneyStore.getState().runs[KEY]).toMatchObject({
        running: true,
        status: {},
      });
      return summary;
    });
    await useJourneyStore.getState().run("c1", "f.journey.yaml");
    expect(useJourneyStore.getState().runs[KEY]).toMatchObject({
      running: false,
      finished: "passed",
    });
  });

  it("reports a run that cannot start", async () => {
    vi.mocked(api.runJourney).mockRejectedValue("no start node");
    await useJourneyStore.getState().run("c1", "f.journey.yaml");
    expect(useJourneyStore.getState().runs[KEY]).toMatchObject({
      running: false,
      finished: "failed",
    });
  });

  it("lists and removes", async () => {
    vi.mocked(api.listJourneys).mockResolvedValue([
      { file: "f.journey.yaml", name: "F", nodes: 2, error: null },
    ]);
    vi.mocked(api.deleteJourney).mockResolvedValue(null);
    await useJourneyStore.getState().load("c1");
    expect(useJourneyStore.getState().byCollection.c1).toHaveLength(1);
    expect(
      await useJourneyStore.getState().remove("c1", "f.journey.yaml"),
    ).toBe(true);
  });
});
