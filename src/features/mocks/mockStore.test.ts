import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import { MAX_LOG_IN_UI, mockKey, useMockStore } from "./mockStore";

vi.mock("./api");

const entry = (id: number) => ({
  id,
  at_ms: 0,
  method: "GET",
  path: "/",
  query: "",
  headers: [],
  body_text: "",
  body_truncated: false,
  remote: "",
  route_id: null,
  response_id: null,
  status: 200,
  duration_ms: 0,
});

describe("mockStore", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useMockStore.setState({ byCollection: {}, logs: {} });
  });

  it("keeps the newest log entries only", () => {
    const store = useMockStore.getState();
    for (let id = 1; id <= MAX_LOG_IN_UI + 5; id += 1)
      store.handleLogged({ collection_id: "c", file: "f", entry: entry(id) });
    const log = useMockStore.getState().logs[mockKey("c", "f")]!;
    expect(log).toHaveLength(MAX_LOG_IN_UI);
    expect(log[log.length - 1]?.id).toBe(MAX_LOG_IN_UI + 5);
  });

  it("reloads the list after start and stop", async () => {
    vi.mocked(api.startMock).mockResolvedValue("u");
    vi.mocked(api.stopMock).mockResolvedValue(null);
    vi.mocked(api.listMocks).mockResolvedValue([]);
    expect(await useMockStore.getState().start("c", "f")).toBe(true);
    await useMockStore.getState().stop("c", "f");
    expect(api.listMocks).toHaveBeenCalledTimes(2);
  });

  it("start reports failure without throwing", async () => {
    vi.mocked(api.startMock).mockRejectedValue({
      code: "invalid_input",
      message: "Port in use",
    });
    vi.mocked(api.listMocks).mockResolvedValue([]);
    expect(await useMockStore.getState().start("c", "f")).toBe(false);
  });
});
