import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "./tauriMock";
import { installWsFake } from "./wsMock";

async function newWsScratch(page: Page) {
  await page.getByRole("button", { name: "Scratch" }).click();
  await page.getByRole("menuitem", { name: "New Request" }).click();
  await page.getByRole("menuitem", { name: "WebSocket" }).click();
  await expect(page.getByRole("tab", { name: "Untitled" })).toBeVisible();
}

const log = (page: Page) => page.getByRole("list", { name: "Message log" }).getByRole("listitem");

const setMessage = async (page: Page, text: string) => {
  await page.getByLabel("Message", { exact: true }).click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText(text);
};

test.describe("websocket", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await installWsFake(page);
    await page.goto("/");
    await newWsScratch(page);
  });

  test("shows the WebSocket sections, a Connect button", async ({ page }) => {
    await expect(page.getByRole("tab", { name: "Messages" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Subprotocols" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Body" })).toHaveCount(0);
    await expect(page.getByLabel("Method")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Connect" })).toBeVisible();
    await expect(page.getByLabel("Connection status")).toHaveText("Idle");
  });

  test("connects, sends JSON, logs the echo with timestamps and disconnects", async ({ page }) => {
    await page.getByLabel("URL").fill("wss://localhost/ws");
    await page.getByRole("button", { name: "Connect" }).click();
    await expect(page.getByLabel("Connection status")).toHaveText("Open");

    await page.getByLabel("Format").selectOption("json");
    await setMessage(page, '{"hello":"world"}');
    await page.getByRole("button", { name: "Send", exact: true }).click();

    const rows = log(page);
    await expect(rows).toHaveCount(2);
    await expect(rows.nth(0)).toContainText('{"hello":"world"}');
    await expect(rows.nth(0)).toContainText(/\d{2}:\d{2}:\d{2}\.\d{3}/);
    await expect(rows.nth(1)).toContainText("echo:");
    await expect(rows.nth(1).getByLabel("Received")).toBeVisible();

    await page.getByRole("button", { name: "Disconnect" }).click();
    await expect(page.getByLabel("Connection status")).toHaveText("Closed");
    await expect(page.getByText(/Closed by you \(1000\)/)).toBeVisible();
  });

  test("shows a message the server pushes on its own and filters by direction", async ({ page }) => {
    await page.getByLabel("URL").fill("wss://localhost/ws");
    await page.getByRole("button", { name: "Connect" }).click();
    await expect(page.getByLabel("Connection status")).toHaveText("Open");
    await page.evaluate(() => (window as unknown as { __wsPush: (t: string) => void }).__wsPush("server hello"));
    await expect(log(page)).toHaveCount(1);
    await expect(log(page).first().getByLabel("Received")).toBeVisible();
    await setMessage(page, "ping");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expect(log(page)).toHaveCount(3);
    await page.getByLabel("Direction").selectOption("sent");
    await expect(log(page)).toHaveCount(1);
  });

  test("shows the backend message when the connection is refused", async ({ page }) => {
    await page.getByLabel("URL").fill("wss://localhost/refuse");
    await page.getByRole("button", { name: "Connect" }).click();
    await expect(page.getByRole("alert")).toContainText("Connection refused");
    await expect(page.getByLabel("Connection status")).toHaveText("Closed");
  });

  test("saves the composed message as a template and loads it back", async ({ page }) => {
    await setMessage(page, "hello there");
    await page.getByLabel("Template name").fill("Greeting");
    await page.getByRole("button", { name: "Save as template" }).click();
    await page.getByRole("tab", { name: /Messages/ }).click();
    await expect(page.getByLabel("Name of Greeting")).toBeVisible();
    await setMessage(page, "something else");
    await page.getByRole("button", { name: "Load Greeting into composer" }).click();
    await expect(page.getByLabel("Message", { exact: true })).toContainText("hello there");
  });
});
