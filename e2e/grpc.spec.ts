import { expect, test, type Page } from "@playwright/test";
import { installGrpcFake } from "./grpcMock";
import { mockTauri } from "./tauriMock";

async function newGrpcScratch(page: Page) {
  await page.getByRole("button", { name: "Scratch" }).click();
  await page.getByRole("menuitem", { name: "New Request" }).click();
  await page.getByRole("menuitem", { name: "gRPC" }).click();
  await expect(page.getByRole("tab", { name: "Untitled" })).toBeVisible();
}

const log = (page: Page) => page.getByRole("list", { name: "Message log" }).getByRole("listitem");
const status = (page: Page) => page.getByLabel("Call status");

async function pick(page: Page, value: string) {
  await expect(page.getByRole("option", { name: /^Say/ })).toHaveCount(1);
  await page.getByLabel("Method", { exact: true }).selectOption(value);
}

const setMessage = async (page: Page, text: string) => {
  await page.getByRole("textbox", { name: "Message" }).click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText(text);
};

test.describe("grpc", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await installGrpcFake(page);
    await page.goto("/");
    await newGrpcScratch(page);
    await page.getByLabel("URL").fill("https://localhost:443");
  });

  test("shows the gRPC sections and a Start button, no HTTP method", async ({ page }) => {
    await expect(page.getByRole("tab", { name: "Message", exact: true })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Metadata" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Protos" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Body" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Start" })).toBeDisabled();
    await expect(page.getByText("TLS", { exact: true })).toBeVisible();
  });

  test("unary: pick a method, template appears, Start shows headers, one message and OK", async ({ page }) => {
    await pick(page, "e2e.Greeter/Say");
    await expect(page.getByRole("textbox", { name: "Message" })).toContainText('"name"');
    await page.getByRole("button", { name: "Start" }).click();

    await expect(page.getByLabel("gRPC status")).toContainText("OK");
    await expect(status(page)).toHaveText("Finished");
    await expect(log(page)).toHaveCount(1);
    await expect(log(page).first()).toContainText("greeting");
    await page.getByRole("tab", { name: "Headers", exact: true }).last().click();
    await expect(page.getByText("x-served-by")).toBeVisible();
    await page.getByRole("tab", { name: "Trailers" }).click();
    await expect(page.getByText("x-trailer")).toBeVisible();
  });

  test("bidi: Start, Send twice, echoes arrive, End finishes the call", async ({ page }) => {
    await pick(page, "e2e.Greeter/Chat");
    await page.getByRole("button", { name: "Start" }).click();
    await expect(status(page)).toHaveText("Running");

    await setMessage(page, '{"name":"one"}');
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await setMessage(page, '{"name":"two"}');
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expect(log(page)).toHaveCount(4);
    await expect(log(page).nth(1)).toContainText("echo");

    await page.getByRole("button", { name: "End", exact: true }).click();
    await expect(page.getByLabel("gRPC status")).toContainText("OK");
    await expect(status(page)).toHaveText("Finished");
    await expect(page.getByRole("button", { name: "Start" })).toBeEnabled();
  });

  test("server stream: Cancel finishes with CANCELLED and no error alert", async ({ page }) => {
    await pick(page, "e2e.Greeter/Watch");
    await page.getByRole("button", { name: "Start" }).click();
    await expect(status(page)).toHaveText("Running");
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(page.getByLabel("gRPC status")).toContainText("CANCELLED");
    await expect(page.getByRole("alert")).toHaveCount(0);
    await expect(status(page)).toHaveText("Finished");
  });

  test("shows the backend message when the connection is refused", async ({ page }) => {
    await pick(page, "e2e.Greeter/Say");
    await page.getByLabel("URL").fill("https://localhost/refuse");
    await page.getByRole("button", { name: "Start" }).click();
    await expect(page.getByRole("alert")).toContainText("Connection refused");
    await expect(status(page)).toHaveText("Finished");
  });
});
