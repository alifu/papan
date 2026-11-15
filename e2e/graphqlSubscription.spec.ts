import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "./tauriMock";
import { installWsFake } from "./wsMock";

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

const results = (page: Page) => page.getByRole("list", { name: "Results" }).getByRole("listitem");

test.describe("graphql subscription", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await installWsFake(page);
    await page.goto("/");
    await newGraphqlScratch(page);
    await page.getByLabel("URL").fill("http://localhost/graphql");
  });

  test("a query operation still shows Send", async ({ page }) => {
    await setQuery(page, "query A { a }");
    await expect(page.getByRole("button", { name: "Send" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Start" })).toHaveCount(0);
  });

  test("starts a subscription, lists results with timestamps and pretty JSON, and stops", async ({ page }) => {
    await setQuery(page, "subscription Ticks { tick }");
    await expect(page.getByRole("button", { name: "Send" })).toHaveCount(0);
    await expect(page.getByLabel("Subscription endpoint")).toHaveText("ws://localhost/graphql");
    await expect(page.getByLabel("Subscription status")).toHaveText("Not started");

    await page.getByRole("button", { name: "Start" }).click();
    await expect(page.getByLabel("Subscription status")).toHaveText("Subscribed");
    await expect(results(page)).toHaveCount(3);
    await expect(results(page).nth(0)).toContainText(/\d{2}:\d{2}:\d{2}\.\d{3}/);
    await expect(results(page).nth(0)).toContainText("next");

    await results(page).nth(2).getByRole("button").click();
    await expect(page.getByLabel(/^Result \d+ body$/)).toContainText('"tick": 3');

    await page.getByRole("button", { name: "Stop" }).click();
    await expect(page.getByLabel("Subscription status")).toHaveText("Stopped");
    await expect(page.getByRole("button", { name: "Start" })).toBeVisible();
  });

  test("shows a rejected start as an error", async ({ page }) => {
    await setQuery(page, "subscription Ticks { tick }");
    await page.getByLabel("URL").fill("http://refuse/graphql");
    await page.getByRole("button", { name: "Start" }).click();
    await expect(page.getByRole("alert")).toContainText("Connection refused");
  });

  test("the Subscription section overrides the URL", async ({ page }) => {
    await setQuery(page, "subscription Ticks { tick }");
    await page.getByRole("tab", { name: "Subscription" }).click();
    await page.getByLabel("WebSocket URL").fill("wss://other/ws");
    await expect(page.getByLabel("Subscription endpoint")).toHaveText("wss://other/ws");
  });
});
