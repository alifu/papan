import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "./tauriMock";

const row = (page: Page, name: string) => page.getByRole("treeitem", { name, exact: true });
const rowNames = (page: Page) =>
  page.getByRole("treeitem").evaluateAll((els) => els.map((e) => e.getAttribute("aria-label")));

async function pick(page: Page, rowName: string, item: string) {
  await row(page, rowName).hover();
  await page.getByRole("menuitem", { name: `Actions for ${rowName}`, exact: true }).click();
  await page.getByRole("menuitem", { name: item, exact: true }).click();
}

async function expandDemo(page: Page) {
  await row(page, "Demo API").click();
  await expect(row(page, "Health")).toBeVisible();
}

test.describe("storage", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await page.goto("/");
  });

  test("renders the seeded collection, creates a request and opens its tab", async ({ page }) => {
    await expect(row(page, "Demo API")).toBeVisible();
    await expandDemo(page);
    await expect(row(page, "Auth")).toBeVisible();

    await pick(page, "Demo API", "New Request");
    await page.getByRole("dialog", { name: "New Request" }).getByLabel("Name").fill("Ping");
    await page.keyboard.press("Enter");

    await expect(row(page, "Ping")).toBeVisible();
    await expect(page.getByRole("tab", { name: "Ping" })).toHaveAttribute("aria-selected", "true");
  });

  test("saves an edited URL and keeps it after closing and reopening the tab", async ({ page }) => {
    await expandDemo(page);
    await row(page, "Health").click();
    const url = page.getByLabel("URL");
    await expect(url).toHaveText("https://api.example.com/health");

    await url.fill("https://api.example.com/v2/health");
    await page.getByRole("button", { name: "Save" }).click();
    await expect(page.getByRole("button", { name: "Save" })).toBeDisabled();
    await page.getByRole("button", { name: "Close Health" }).click();
    await row(page, "Health").click();

    await expect(page.getByLabel("URL")).toHaveText("https://api.example.com/v2/health");
  });

  test("renames and deletes a request", async ({ page }) => {
    await expandDemo(page);

    await pick(page, "Health", "Rename");
    await page.getByRole("textbox", { name: "Name" }).fill("Ping");
    await page.keyboard.press("Enter");
    await expect(row(page, "Ping")).toBeVisible();
    await expect(row(page, "Health")).toHaveCount(0);

    await pick(page, "Ping", "Delete");
    await page.getByRole("dialog").getByRole("button", { name: "Delete" }).click();
    await expect(row(page, "Ping")).toHaveCount(0);
  });

  test("Move Down changes the row order", async ({ page }) => {
    await expandDemo(page);
    expect(await rowNames(page)).toEqual(["Demo API", "Health", "Auth"]);

    await pick(page, "Health", "Move Down");

    await expect.poll(() => rowNames(page)).toEqual(["Demo API", "Auth", "Health"]);
  });

  test("moves a scratch request into a collection folder and keeps its tab open", async ({ page }) => {
    await page.getByRole("button", { name: "Scratch" }).click();
    await page.getByRole("button", { name: /Quick check/ }).click();
    await expect(page.getByRole("tab", { name: "Quick check" })).toBeVisible();

    await page.getByRole("menuitem", { name: "Actions for Quick check" }).click();
    await page.getByRole("menuitem", { name: "Move to collection…" }).click();
    await page.getByLabel("Folder").selectOption("auth");
    await page.getByRole("button", { name: "Move" }).click();

    await expect(page.getByText("No scratch requests yet.")).toBeVisible();
    await expect(page.getByRole("tab", { name: "Quick check" })).toBeVisible();
    await page.getByRole("button", { name: "Collections" }).click();
    await expandDemo(page);
    await row(page, "Auth").click();
    await expect(row(page, "Quick check")).toBeVisible();
  });

  test("shows the backend message when creating a request fails", async ({ page }) => {
    await expandDemo(page);
    await page.evaluate(() => window.__failNext("create_request"));

    await pick(page, "Demo API", "New Request");
    await page.getByRole("textbox", { name: "Name" }).fill("Nope");
    await page.keyboard.press("Enter");

    await expect(page.getByRole("alert")).toHaveText(/Backend says no\./);
  });
});
