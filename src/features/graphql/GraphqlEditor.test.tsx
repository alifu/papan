import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GraphqlSchema } from "../../ipc/bindings";
import { scratchLocation } from "../locations";
import { emptyContent } from "../request/content";
import * as api from "./api";
import { GraphqlEditor } from "./GraphqlEditor";
import { useGraphqlStore } from "./graphqlStore";

vi.mock("./api");
vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));

const loc = scratchLocation("s1");
const schema: GraphqlSchema = {
  endpoint: "http://x/graphql",
  fetched_at_ms: Date.UTC(2026, 0, 1, 12, 4),
  source: { type: "sdl", text: "type Query { a: Int }" },
};
const gqlContent = (query: string, operation_name = "") => ({
  ...emptyContent(),
  kind: "graphql" as const,
  body: { type: "graphql" as const, query, variables: "", operation_name },
});

const mount = (content = gqlContent("{ a }"), onChange = vi.fn()) => {
  render(<GraphqlEditor tabId="t" location={loc} content={content} onChange={onChange} />);
  return onChange;
};

describe("GraphqlEditor", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useGraphqlStore.setState({ byTab: {} });
    vi.mocked(api.readCachedSchema).mockResolvedValue(null);
  });

  it("edits the query", async () => {
    const onChange = mount(gqlContent(""));
    await userEvent.type(screen.getByLabelText("GraphQL query"), "{{");
    expect(onChange).toHaveBeenCalledWith({ type: "graphql", query: "{", variables: "", operation_name: "" });
  });

  it("starts without a schema and fetches it on demand", async () => {
    vi.mocked(api.introspectGraphql).mockResolvedValue(schema);
    mount();
    expect(screen.getByRole("status")).toHaveTextContent("No schema loaded");
    expect(screen.getByRole("button", { name: "Explorer" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Fetch schema" }));
    expect(await screen.findByRole("button", { name: "Refresh" })).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent(/^Schema from /);
    expect(api.introspectGraphql).toHaveBeenCalledWith(expect.any(String), loc, expect.objectContaining({ kind: "graphql" }));
  });

  it("loads the cached schema when it opens and shows the explorer on request", async () => {
    vi.mocked(api.readCachedSchema).mockResolvedValue(schema);
    mount();
    await screen.findByRole("button", { name: "Refresh" });
    await userEvent.click(screen.getByRole("button", { name: "Explorer" }));
    expect(screen.getByRole("complementary", { name: "Schema explorer" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Explorer" }));
    expect(screen.queryByRole("complementary", { name: "Schema explorer" })).toBeNull();
  });

  it("shows the error of a failed fetch", async () => {
    vi.mocked(api.introspectGraphql).mockRejectedValue({ code: "network", message: "Could not connect." });
    mount();
    await userEvent.click(screen.getByRole("button", { name: "Fetch schema" }));
    expect(await screen.findByText("Could not connect.")).toBeVisible();
  });

  it("reports a schema that cannot be built", async () => {
    vi.mocked(api.readCachedSchema).mockResolvedValue({ ...schema, source: { type: "sdl", text: "type {" } });
    mount();
    expect(await screen.findByText(/The schema could not be read/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Explorer" })).toBeDisabled();
  });

  it("loads a schema from a file and clears it", async () => {
    vi.mocked(api.loadSchemaFile).mockResolvedValue({ ...schema, fetched_at_ms: null });
    vi.mocked(api.clearCachedSchema).mockResolvedValue(null);
    mount();
    await userEvent.click(screen.getByRole("button", { name: "Load from file…" }));
    expect(await screen.findByText("Schema from file")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Clear schema" }));
    expect(await screen.findByText("No schema loaded")).toBeVisible();
    expect(api.clearCachedSchema).toHaveBeenCalled();
  });

  it("offers no operation picker for a single operation", async () => {
    mount(gqlContent("query A { a }"));
    expect(screen.queryByLabelText("Operation")).toBeNull();
  });

  it("lists several operations and writes the picked name", async () => {
    const onChange = mount(gqlContent("query A { a } query B { a }", "A"));
    expect(screen.getByLabelText("Operation")).toHaveValue("A");
    await userEvent.selectOptions(screen.getByLabelText("Operation"), "B");
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ operation_name: "B" }));
  });
});
