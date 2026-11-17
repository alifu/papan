import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GraphqlSchema } from "../../ipc/bindings";
import { scratchLocation } from "../locations";
import { emptyContent } from "../request/content";
import * as api from "./api";
import { graphqlTab, useGraphqlStore } from "./graphqlStore";

vi.mock("./api");

const loc = scratchLocation("s1");
const content = emptyContent();
const schema: GraphqlSchema = { endpoint: "http://x/graphql", fetched_at_ms: 5, source: { type: "sdl", text: "type Query { a: Int }" } };
const state = () => graphqlTab(useGraphqlStore.getState(), "t");
const store = () => useGraphqlStore.getState();

describe("graphqlStore", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useGraphqlStore.setState({ byTab: {} });
  });

  it("is idle until something loads", () => {
    expect(state().status).toBe("idle");
  });

  it("loads the cached schema, and stays idle on a miss or error", async () => {
    vi.mocked(api.readCachedSchema).mockResolvedValueOnce(null);
    await store().loadCached("t", loc, content);
    expect(state().status).toBe("idle");

    vi.mocked(api.readCachedSchema).mockRejectedValueOnce({ code: "invalid_input", message: "bad url" });
    await store().loadCached("t", loc, content);
    expect(state().status).toBe("idle");

    vi.mocked(api.readCachedSchema).mockResolvedValueOnce(schema);
    await store().loadCached("t", loc, content);
    expect(state()).toMatchObject({ status: "ready", schema, endpoint: "http://x/graphql", fetchedAt: 5 });
  });

  it("introspects, going through loading", async () => {
    let resolve: (s: GraphqlSchema) => void = () => {};
    vi.mocked(api.introspectGraphql).mockReturnValue(new Promise((r) => (resolve = r)));
    const pending = store().introspect("t", loc, content);
    expect(state().status).toBe("loading");
    resolve(schema);
    await pending;
    expect(state()).toMatchObject({ status: "ready", requestId: null });
  });

  it("keeps the error text of a failed introspection", async () => {
    vi.mocked(api.introspectGraphql).mockRejectedValue({ code: "network", message: "Could not connect." });
    await store().introspect("t", loc, content);
    expect(state()).toMatchObject({ status: "error", error: "Could not connect." });
  });

  it("returns to idle when cancelled and keeps an earlier schema", async () => {
    vi.mocked(api.introspectGraphql).mockRejectedValue({ code: "cancelled", message: "Cancelled" });
    await store().introspect("t", loc, content);
    expect(state()).toMatchObject({ status: "idle", error: null });

    vi.mocked(api.readCachedSchema).mockResolvedValue(schema);
    await store().loadCached("t", loc, content);
    await store().introspect("t", loc, content);
    expect(state().status).toBe("ready");
  });

  it("cancels the running introspection", async () => {
    vi.mocked(api.cancelIntrospection).mockResolvedValue(null);
    vi.mocked(api.introspectGraphql).mockReturnValue(new Promise(() => {}));
    void store().introspect("t", loc, content);
    store().cancel("t");
    expect(api.cancelIntrospection).toHaveBeenCalledWith(state().requestId);
  });

  it("loads a file, ignoring a cancelled dialog", async () => {
    vi.mocked(api.loadSchemaFile).mockResolvedValueOnce(null);
    await store().loadFile("t", loc, content);
    expect(state().status).toBe("idle");
    vi.mocked(api.loadSchemaFile).mockResolvedValueOnce(schema);
    await store().loadFile("t", loc, content);
    expect(state().schema).toBe(schema);
    vi.mocked(api.loadSchemaFile).mockRejectedValueOnce({ code: "invalid_input", message: "Not a schema." });
    await store().loadFile("t", loc, content);
    expect(state()).toMatchObject({ status: "error", error: "Not a schema." });
  });

  it("clears the schema", async () => {
    vi.mocked(api.readCachedSchema).mockResolvedValue(schema);
    vi.mocked(api.clearCachedSchema).mockResolvedValue(null);
    await store().loadCached("t", loc, content);
    await store().clear("t", loc, content);
    expect(state()).toMatchObject({ status: "idle", schema: null });
  });

  it("forgets a closed tab", async () => {
    vi.mocked(api.readCachedSchema).mockResolvedValue(schema);
    await store().loadCached("t", loc, content);
    store().drop("t");
    expect(useGraphqlStore.getState().byTab).toEqual({});
  });
});
