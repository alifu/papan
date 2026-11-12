import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "./tauriMock";

async function openScratch(page: Page) {
  await page.getByRole("button", { name: "Scratch" }).click();
  await page.getByRole("button", { name: /Quick check/ }).click();
  await expect(page.getByRole("tab", { name: "Quick check" })).toBeVisible();
}

test.describe("http", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await page.goto("/");
    await openScratch(page);
  });

  test("keeps the URL and the params table in sync and shows the response", async ({ page }) => {
    const url = page.getByLabel("URL");
    await url.fill("http://localhost/json?a=1");
    await expect(page.getByLabel("Name 1")).toHaveValue("a");

    await page.getByLabel("Name 2").fill("b");
    await page.getByLabel("Value 2").fill("2");
    await expect(url).toHaveValue("http://localhost/json?a=1&b=2");

    await page.getByLabel("Enable row 1").uncheck();
    await expect(url).toHaveValue("http://localhost/json?b=2");

    await page.getByRole("tab", { name: /Headers/ }).click();
    await page.getByLabel("Name 1").fill("Accept");
    await page.getByLabel("Value 1").fill("application/json");
    await url.focus();
    await page.keyboard.press("ControlOrMeta+Enter");

    await expect(page.getByText("200")).toBeVisible();
    await expect(page.getByRole("tab", { name: /Headers/ }).last()).toBeVisible();
  });

  test("marks dirty tabs, saves with the shortcut and asks before closing", async ({ page }) => {
    await page.getByLabel("URL").fill("http://localhost/json");
    await expect(page.getByRole("tab", { name: /•.*Quick check|Quick check.*•/ })).toBeVisible();

    await page.keyboard.press("ControlOrMeta+s");
    await expect(page.getByRole("button", { name: "Save" })).toBeDisabled();
    await expect(page.getByRole("tab", { name: "Quick check" })).toBeVisible();

    await page.getByLabel("URL").fill("http://localhost/json2");
    await page.getByRole("button", { name: "Close Quick check" }).click();
    const dialog = page.getByRole("dialog", { name: "Discard changes?" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Discard" }).click();
    await expect(page.getByRole("tab", { name: "Quick check" })).toHaveCount(0);
  });

  test("cancels a slow request and shows backend errors", async ({ page }) => {
    await page.getByLabel("URL").fill("http://localhost/slow");
    await page.getByRole("button", { name: "Send" }).click();
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(page.getByText("Cancelled")).toBeVisible();

    await page.getByLabel("URL").fill("http://localhost/error");
    await page.getByRole("button", { name: "Send" }).click();
    await expect(page.getByText("Could not connect to localhost.")).toBeVisible();
  });

  test("previews html in a sandboxed iframe and images as <img>", async ({ page }) => {
    await page.getByLabel("URL").fill("http://localhost/html");
    await page.getByRole("button", { name: "Send" }).click();
    await page.getByRole("button", { name: "Preview" }).click();
    await expect(page.locator('iframe[sandbox=""]')).toBeVisible();

    await page.getByLabel("URL").fill("http://localhost/image");
    await page.getByRole("button", { name: "Send" }).click();
    await expect(page.locator('img[src^="data:image/png"]')).toBeVisible();
  });

  test("lists sends in History and reopens one as a scratch tab", async ({ page }) => {
    await page.getByLabel("URL").fill("http://localhost/json");
    await page.getByRole("button", { name: "Send" }).click();
    await expect(page.getByText("200")).toBeVisible();

    await page.getByRole("button", { name: "History" }).click();
    await page.getByRole("button", { name: /localhost\/json/ }).click();
    await expect(page.getByRole("tablist", { name: "Open tabs" }).getByRole("tab")).toHaveCount(3);
  });
});
