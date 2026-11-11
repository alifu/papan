import { test, expect } from "@playwright/test";

test("home page shows main content", async ({ page }) => {
  await page.goto("/");

  await expect(page.getByText("Main Content")).toBeVisible();
});
