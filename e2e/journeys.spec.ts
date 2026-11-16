import { expect, test } from "@playwright/test";
import { mockTauri } from "./tauriMock";

const SHOTS = process.env.JOURNEY_SHOTS ?? "";
const shot = async (page: import("@playwright/test").Page, name: string) => {
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/${name}.png` });
};

test.describe("journeys", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await page.goto("/");
  });

  test("builds a journey on the canvas, deletes a node with the keyboard and runs it", async ({ page }) => {
    await page.getByRole("button", { name: "Journeys" }).click();
    await page.getByRole("button", { name: "New journey in Demo API" }).click();
    const canvas = page.getByLabel("Journey canvas");
    await expect(canvas.locator(".react-flow__node")).toHaveCount(1);
    await shot(page, "1-new");

    await page.getByRole("button", { name: "Add Delay node" }).click();
    await page.getByRole("button", { name: "Add End node" }).click();
    await expect(canvas.locator(".react-flow__node")).toHaveCount(3);
    await shot(page, "2-added");

    // Select by click, then the Delete key removes it.
    await canvas.locator(".react-flow__node", { hasText: "Delay" }).click();
    await expect(page.getByRole("button", { name: "Delete node" })).toBeVisible();
    await page.keyboard.press("Delete");
    await expect(canvas.locator(".react-flow__node")).toHaveCount(2);
    await shot(page, "3-deleted");

    // The Start node cannot be deleted.
    await canvas.locator(".react-flow__node", { hasText: "Start" }).click();
    await expect(page.getByRole("button", { name: "Delete node" })).toHaveCount(0);
    await page.keyboard.press("Delete");
    await expect(canvas.locator(".react-flow__node")).toHaveCount(2);

    // Connect Start to End by dragging between handles.
    const from = canvas.locator(".react-flow__node", { hasText: "Start" }).locator(".react-flow__handle");
    const to = canvas.locator(".react-flow__node", { hasText: "End" }).locator(".react-flow__handle");
    await from.hover();
    await page.mouse.down();
    await to.hover();
    await page.mouse.up();
    await expect(canvas.locator(".react-flow__edge")).toHaveCount(1);
    await shot(page, "4-connected");

    // An edge can be selected and deleted too.
    await canvas.locator(".react-flow__edge").click({ force: true });
    await page.getByRole("button", { name: "Delete edge" }).click();
    await expect(canvas.locator(".react-flow__edge")).toHaveCount(0);
    await from.hover();
    await page.mouse.down();
    await to.hover();
    await page.mouse.up();
    await expect(canvas.locator(".react-flow__edge")).toHaveCount(1);

    await page.getByRole("button", { name: "Tidy up" }).click();
    await page.getByRole("button", { name: "Run" }).click();
    await expect(canvas.getByRole("img", { name: "Passed" })).toHaveCount(2);
    await expect(canvas.getByText("×2")).toHaveCount(0);
    await expect(page.getByText("Run passed (2 steps)")).toBeVisible();
    await shot(page, "5-ran");
  });
});
