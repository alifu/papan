import { expect, test } from "@playwright/test";
import { mockTauri } from "./tauriMock";

test.describe("webhooks", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await page.goto("/");
  });

  test("creates a listener, starts it, shows a live capture and replays it into a scratch tab", async ({ page }) => {
    await page.getByRole("button", { name: "Webhooks" }).click();
    await expect(page.getByText("No webhook listeners yet.")).toBeVisible();
    await page.getByRole("button", { name: "New Listener" }).click();

    await expect(page.getByRole("tab", { name: "Webhook", exact: true })).toBeVisible();
    await expect(page.getByText("No requests captured yet.")).toBeVisible();

    await page.getByRole("button", { name: "Start", exact: true }).click();
    await expect(page.getByText("http://127.0.0.1:9123/hooks")).toBeVisible();
    await expect(page.getByText(/ngrok http 9123/)).toBeVisible();
    await expect(page.getByRole("img", { name: "Running" })).toBeVisible();

    await page.evaluate(() => window.__webhookCapture("w1", { method: "POST", path: "/hooks", body: '{"event":"paid"}' }));
    const list = page.getByRole("list", { name: "Captured requests" });
    await expect(list.getByRole("button", { name: /POST/ })).toBeVisible();

    await list.getByRole("button", { name: /POST/ }).click();
    const detail = page.getByRole("region", { name: "Capture detail" });
    await expect(detail.getByText("X-Signature")).toBeVisible();
    await expect(detail.getByLabel("Body")).toContainText('"event": "paid"');

    await detail.getByRole("button", { name: "Replay as new request" }).click();
    await expect(page.getByRole("tab", { name: /POST \/hooks/ })).toBeVisible();
    await expect(page.getByLabel("URL")).toBeVisible();
  });

  test("validates the settings form and saves a valid one", async ({ page }) => {
    await page.getByRole("button", { name: "Webhooks" }).click();
    await page.getByRole("button", { name: "New Listener" }).click();
    await page.getByRole("tab", { name: "Settings" }).click();

    await page.getByLabel("Path", { exact: true }).fill("nope");
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("Path must start with /.");

    await page.getByLabel("Path", { exact: true }).fill("/stripe");
    await page.getByLabel("Name", { exact: true }).fill("Stripe");
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page.getByRole("tab", { name: "Stripe", exact: true })).toBeVisible();
  });

  test("stops a listener from the sidebar and deletes it", async ({ page }) => {
    await page.getByRole("button", { name: "Webhooks" }).click();
    await page.getByRole("button", { name: "New Listener" }).click();
    await page.getByRole("button", { name: "Start Webhook" }).click();
    await expect(page.getByRole("img", { name: "Running" })).toBeVisible();
    await page.getByRole("button", { name: "Stop Webhook" }).click();
    await expect(page.getByRole("img", { name: "Stopped" })).toBeVisible();

    await page.getByRole("menuitem", { name: "Actions for Webhook" }).click();
    await page.getByRole("menuitem", { name: "Delete" }).click();
    await expect(page.getByText("No webhook listeners yet.")).toBeVisible();
    await expect(page.getByRole("tab", { name: "Webhook", exact: true })).toHaveCount(0);
  });
});
