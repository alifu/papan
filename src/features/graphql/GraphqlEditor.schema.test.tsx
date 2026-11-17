import { render } from "@testing-library/react";
import { EditorView } from "@codemirror/view";
import { getSchema } from "cm6-graphql";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GraphqlSchema } from "../../ipc/bindings";
import { scratchLocation } from "../locations";
import { emptyContent } from "../request/content";
import * as api from "./api";
import { GraphqlEditor } from "./GraphqlEditor";
import { useGraphqlStore } from "./graphqlStore";

vi.mock("./api");

// Uses the real CodeEditor: proves the schema reaches cm6-graphql (completion and lint read it from state).
const schema: GraphqlSchema = {
  endpoint: "http://x/graphql",
  fetched_at_ms: 1,
  source: { type: "sdl", text: "type Query { a: Int }" },
};

describe("GraphqlEditor with the real editor", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useGraphqlStore.setState({ byTab: {} });
  });

  it("hands the built schema to the editor once it is loaded", async () => {
    vi.mocked(api.readCachedSchema).mockResolvedValue(schema);
    const content = { ...emptyContent(), kind: "graphql" as const };
    const { findByRole } = render(
      <GraphqlEditor tabId="t" location={scratchLocation("s1")} content={content} onChange={() => {}} />,
    );
    await findByRole("button", { name: "Refresh" });
    const view = EditorView.findFromDOM(document.querySelector(".cm-editor") as HTMLElement)!;
    expect(getSchema(view.state)?.getQueryType()?.name).toBe("Query");
  });
});
