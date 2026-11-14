import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "./tauriMock";

async function newGraphqlScratch(page: Page) {
  await page.getByRole("button", { name: "Scratch" }).click();
  await page.getByRole("menuitem", { name: "New Request" }).click();
  await page.getByRole("menuitem", { name: "GraphQL" }).click();
  await expect(page.getByRole("tab", { name: "Untitled" })).toBeVisible();
  await page.getByRole("tab", { name: "Query", exact: true }).click();
}

const setQuery = async (page: Page, text: string) => {
  await page.getByLabel("GraphQL query").click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText(text);
};

test.describe("graphql", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await page.goto("/");
    await newGraphqlScratch(page);
    await page.getByLabel("URL").fill("http://localhost/json");
  });

  test("shows the GraphQL sections and no method select", async ({ page }) => {
    await expect(page.getByRole("tab", { name: "Variables" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Body" })).toHaveCount(0);
    await expect(page.getByLabel("Method")).toHaveCount(0);
  });

  test("fetches the schema, opens the explorer and clears it", async ({ page }) => {
    await expect(page.getByRole("status").filter({ hasText: "No schema loaded" })).toBeVisible();
    await page.getByRole("button", { name: "Fetch schema" }).click();
    await expect(page.getByText(/^Schema from /)).toBeVisible();

    await page.getByRole("button", { name: "Explorer" }).click();
    const explorer = page.getByRole("complementary", { name: "Schema explorer" });
    await explorer.getByRole("button", { name: "Query", exact: true }).click();
    await expect(explorer.getByRole("heading", { name: "Query" })).toBeVisible();
    await explorer.getByLabel("Search schema").fill("writer");
    await expect(explorer.getByRole("button", { name: "Post.writer" })).toBeVisible();

    await page.getByRole("button", { name: "Clear schema" }).click();
    await expect(page.getByText("No schema loaded")).toBeVisible();
  });

  test("loads a schema from a file and shows a fetch failure", async ({ page }) => {
    await page.getByRole("button", { name: "Load from file…" }).click();
    await expect(page.getByText("Schema from file")).toBeVisible();

    await page.getByLabel("URL").fill("http://localhost/down");
    await page.getByRole("button", { name: "Refresh" }).click();
    await expect(page.getByText("Could not connect to localhost.")).toBeVisible();
  });

  test("picks between operations, sends the selection and blocks subscriptions", async ({ page }) => {
    await setQuery(page, "query A { user(id: 1) { id } } query B { search(text: \"x\") { __typename } }");
    await page.getByLabel("Operation").selectOption("B");
    await page.getByRole("tab", { name: "Variables" }).click();
    await page.getByLabel("GraphQL variables").click();
    await page.keyboard.insertText("[1]");
    await expect(page.getByText("Variables should be a JSON object.")).toBeVisible();
    await page.keyboard.press("ControlOrMeta+a");
    await page.keyboard.insertText('{"id": "1"}');

    await page.getByRole("button", { name: "Send" }).click();
    await expect(page.getByText("200")).toBeVisible();
    const sent = await page.evaluate(() => window.__sent.at(-1));
    expect(sent).toMatchObject({ kind: "graphql", body: { type: "graphql", operation_name: "B", variables: '{"id": "1"}' } });

    await page.getByRole("tab", { name: "Query", exact: true }).click();
    await setQuery(page, "subscription S { tick }");
    await expect(page.getByRole("button", { name: "Send" })).toBeDisabled();
    await expect(page.getByText(/Subscriptions arrive with the WebSocket engine/)).toBeVisible();
  });
});
