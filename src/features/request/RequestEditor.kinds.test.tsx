import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as collections from "../collections/api";
import * as graphqlApi from "../graphql/api";
import { scratchLocation } from "../locations";
import * as api from "./api";
import { emptyContent } from "./content";
import { RequestEditor } from "./RequestEditor";
import { useRequestStore } from "./requestStore";

vi.mock("../collections/api");
vi.mock("./api");
vi.mock("../graphql/api");
vi.mock("../environments/api");
vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));
vi.mock("../response/ResponseViewer", () => ({ ResponseViewer: () => <div>viewer</div> }));
vi.mock("../soap/SoapPanel", () => ({
  default: ({ onChange }: { onChange: (u: (c: { url: string }) => { url: string }) => void }) => (
    <button onClick={() => onChange((c) => ({ ...c, url: "http://soap" }))}>soap-panel</button>
  ),
}));

const loc = scratchLocation("s1");
const gql = (query: string, operation_name = "") => ({
  ...emptyContent(),
  kind: "graphql" as const,
  url: "http://x/graphql",
  method: "POST",
  body: { type: "graphql" as const, query, variables: "", operation_name },
});
const open = async (content: ReturnType<typeof emptyContent>) => {
  vi.mocked(collections.readRequest).mockResolvedValue({ name: "R", content });
  render(<RequestEditor tabId="t" location={loc} />);
  await screen.findByLabelText("URL");
};
const tabNames = () => screen.getAllByRole("tab").map((t) => t.textContent);

describe("RequestEditor by kind", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useRequestStore.setState({ byTab: {} });
    vi.mocked(graphqlApi.readCachedSchema).mockResolvedValue(null);
  });

  it("shows Query and Variables, and no Body or method, for GraphQL", async () => {
    await open(gql("{ a }"));
    expect(tabNames().slice(0, 4)).toEqual(["Params", "Headers", "Query", "Variables"]);
    expect(screen.queryByRole("tab", { name: "Body" })).toBeNull();
    expect(screen.queryByLabelText("Method")).toBeNull();
    await userEvent.click(screen.getByRole("tab", { name: "Query" }));
    expect(screen.getByLabelText("GraphQL query")).toHaveValue("{ a }");
    await userEvent.click(screen.getByRole("tab", { name: "Variables" }));
    await userEvent.type(screen.getByLabelText("GraphQL variables"), "1");
    expect(useRequestStore.getState().byTab.t?.draft?.body).toMatchObject({ type: "graphql", variables: "1" });
  });

  it("disables Send with a hint for a subscription and never sends it", async () => {
    await open(gql("subscription S { s }"));
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(screen.getByText(/Subscriptions arrive with the WebSocket engine/)).toBeVisible();
    await useRequestStore.getState().send("t", loc);
    expect(api.sendRequestOutcome).not.toHaveBeenCalled();
  });

  it("enables Send once a query operation is selected", async () => {
    await open(gql("query A { a } subscription S { s }", "A"));
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  });

  it("shows Body (XML only) and SOAP for SOAP and applies the panel's update", async () => {
    await open({ ...emptyContent(), kind: "soap", method: "POST", body: { type: "xml", text: "<a/>" } });
    expect(tabNames().slice(0, 4)).toEqual(["Params", "Headers", "Body", "SOAP"]);
    await userEvent.click(screen.getByRole("tab", { name: "Body" }));
    expect(within(screen.getByLabelText("Body type")).getAllByRole("option").map((o) => o.textContent)).toEqual(["XML"]);
    await userEvent.click(screen.getByRole("tab", { name: "SOAP" }));
    await userEvent.click(screen.getByRole("button", { name: "soap-panel" }));
    expect(useRequestStore.getState().byTab.t?.draft?.url).toBe("http://soap");
  });
});
