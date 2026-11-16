import type { Page } from "@playwright/test";

/**
 * In-memory fake of the M13 journey commands (`list_journeys`, `read_journey`, `save_journey`, `delete_journey`,
 * `run_journey`, `cancel_journey`). `run_journey` walks the saved nodes in order and emits `journey-run-event`s, so the
 * canvas shows live status. Must run after the base fake has published `window.__handlers`.
 */
export async function installJourneyFake(page: Page): Promise<void> {
  await page.addInitScript(() => {
    type Args = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
    const w = window as unknown as { __handlers: Record<string, (a: Args) => any>; __emit: (e: string, p: unknown) => void }; // eslint-disable-line @typescript-eslint/no-explicit-any
    const files = new Map<string, Args>();
    const summary = (file: string, journey: Args) => ({ file, name: journey.name, nodes: journey.nodes.length, error: null });

    Object.assign(w.__handlers, {
      list_journeys: () => [...files.entries()].map(([file, journey]) => summary(file, journey)),
      read_journey: (a: Args) => files.get(a.file),
      save_journey: (a: Args) => {
        const file = a.file ?? `${String(a.journey.name).toLowerCase().replace(/[^a-z0-9]+/g, "-")}.journey.yaml`;
        files.set(file, JSON.parse(JSON.stringify(a.journey)));
        return summary(file, a.journey);
      },
      delete_journey: (a: Args) => {
        files.delete(a.file);
        return null;
      },
      cancel_journey: () => null,
      run_journey: (a: Args) => {
        const journey = files.get(a.file) as Args;
        const base = { status: null, message: "", response: null, vars: [], console: [], run_status: null };
        const emit = (event: Args) =>
          w.__emit("journey-run-event", { run_id: "r1", collection_id: a.collectionId, file: a.file, event: { ...base, ...event } });
        let steps = 0;
        for (const node of journey.nodes as Args[]) {
          steps += 1;
          emit({ kind: "node_started", node_id: node.id, steps });
          emit({ kind: "node_finished", node_id: node.id, status: "passed", steps });
        }
        emit({ kind: "run_finished", node_id: "", run_status: "passed", steps });
        return { run_id: "r1", status: "passed", steps, failures: 0, message: "" };
      },
    });
  });
}
