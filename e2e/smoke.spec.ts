import { test, expect } from "@playwright/test";
import { mockTauri } from "./tauriMock";

test.describe("shell", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
  });

  test("renders the shell", async ({ page }) => {
    await page.goto("/");

    await expect(page.getByRole("menubar")).toBeVisible();
    const nav = page.getByRole("navigation", { name: "Navigation" });
    for (const name of ["Collections", "Scratch", "Mocks", "Journeys", "History"]) {
      await expect(nav.getByRole("button", { name })).toBeVisible();
    }
    await expect(page.getByRole("tab", { name: "Welcome" })).toBeVisible();
    await expect(page.getByRole("contentinfo")).toContainText("Papan 0.0.0-e2e");
  });

  test("switches and persists the theme", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "dark" });
    await page.goto("/");

    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(page.locator("body")).toHaveCSS("background-color", "rgb(0, 0, 0)");
    await page.screenshot({ path: "test-results/shell-dark.png", fullPage: true });

    await page.getByRole("radio", { name: "Light" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await page.screenshot({ path: "test-results/shell-light.png", fullPage: true });

    await page.reload();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  });

  test("resizes and collapses the sidebar", async ({ page }) => {
    await page.goto("/");
    const separator = page.getByRole("separator", { name: "Resize sidebar" });
    const before = Number(await separator.getAttribute("aria-valuenow"));

    await separator.focus();
    await page.keyboard.press("ArrowRight");
    await expect(separator).toHaveAttribute("aria-valuenow", String(before + 16));

    await page.getByRole("button", { name: "Collapse sidebar" }).click();
    await expect(page.getByRole("button", { name: "Expand sidebar" })).toBeVisible();

    await page.reload();
    await expect(page.getByRole("button", { name: "Expand sidebar" })).toBeVisible();
  });

  test("opens and closes tabs", async ({ page }) => {
    await page.goto("/");

    await page.getByRole("toolbar", { name: "Main" }).getByRole("button", { name: "New Tab" }).click();
    await expect(page.getByRole("tab", { name: "Untitled" })).toHaveAttribute("aria-selected", "true");

    await page.getByRole("button", { name: "Close Untitled" }).click();
    await expect(page.getByRole("tab", { name: "Welcome" })).toHaveAttribute("aria-selected", "true");
  });
});

test("shows a generic error in the status bar without the Tauri runtime", async ({ page }) => {
  await page.goto("/");

  await expect(page.getByRole("contentinfo").getByRole("alert")).toHaveText("Something went wrong. Please try again.");
});
