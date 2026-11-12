import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "./tauriMock";

const row = (page: Page, name: string) => page.getByRole("treeitem", { name, exact: true });

async function openHealth(page: Page) {
  await row(page, "Demo API").click();
  await row(page, "Health").click();
  await expect(page.getByLabel("URL")).toBeVisible();
}

test.describe("environments", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await page.goto("/");
  });

  test("creates an environment, selects it and sends with its variable", async ({ page }) => {
    await openHealth(page);
    await page.getByRole("button", { name: "Variables…" }).click();
    const dialog = page.getByRole("dialog", { name: "Variables" });
    await dialog.getByRole("button", { name: "New environment" }).click();
    await page.getByRole("dialog", { name: "New Environment" }).getByLabel("Name").fill("QA");
    await page.getByRole("button", { name: "Create" }).click();

    await dialog.getByLabel("Variable name 1").fill("path");
    await dialog.getByLabel("Variable value 1").fill("json");
    await dialog.getByRole("button", { name: "Save" }).click();
    await dialog.getByRole("button", { name: "Cancel" }).click();

    await page.getByLabel("Environment").selectOption({ label: "QA" });
    await page.getByLabel("URL").fill("http://localhost/{{path}}");
    await page.getByLabel("URL").focus();
    await page.keyboard.press("ControlOrMeta+Enter");
    await expect(page.getByText("200 OK")).toBeVisible();
  });

  test("styles tokens by state and reports unresolved names after a send", async ({ page }) => {
    await openHealth(page);
    await page.getByLabel("URL").fill("http://localhost/{{path}}");
    await expect(page.locator(".var-ok")).toHaveText("{{path}}");

    await page.getByLabel("Environment").selectOption({ label: "No environment" });
    await expect(page.locator(".var-missing")).toHaveText("{{path}}");

    await page.getByRole("button", { name: "Send" }).click();
    await expect(page.getByText(/Sent with unresolved variables: path/)).toBeVisible();
  });

  test("keeps secret values write-only", async ({ page }) => {
    await openHealth(page);
    await page.getByRole("button", { name: "Variables…" }).click();
    const dialog = page.getByRole("dialog", { name: "Variables" });
    await dialog.getByRole("button", { name: "Dev", exact: true }).click();
    await expect(dialog.getByLabel("Variable name 1")).toHaveValue("api_token");

    await dialog.getByLabel("Secret value 1").fill("s3cr3t-value");
    await dialog.getByRole("button", { name: "Save" }).click();
    await dialog.getByRole("button", { name: "Cancel" }).click();

    await page.getByRole("button", { name: "Variables…" }).click();
    await page.getByRole("dialog", { name: "Variables" }).getByRole("button", { name: "Dev", exact: true }).click();
    const secret = page.getByLabel("Secret value 1");
    await expect(secret).toHaveValue("");
    await expect(secret).toHaveAttribute("placeholder", "Stored");
    expect(await page.content()).not.toContain("s3cr3t-value");
  });

  test("disables the switcher for scratch tabs and Mod+Enter still sends from the body editor", async ({ page }) => {
    await page.getByRole("button", { name: "Scratch" }).click();
    await page.getByRole("button", { name: /Quick check/ }).click();
    await expect(page.getByLabel("Environment")).toBeDisabled();

    await page.getByLabel("URL").fill("http://localhost/json");
    await page.getByRole("tab", { name: "Body" }).click();
    await page.getByLabel("Body type").selectOption("json");
    await page.getByLabel("Body text").click();
    await page.keyboard.type('{"a":1}');
    await page.keyboard.press("ControlOrMeta+Enter");
    await expect(page.getByText("200 OK")).toBeVisible();
  });

  test("saves a bearer token as a secret variable", async ({ page }) => {
    await openHealth(page);
    await page.getByRole("tab", { name: "Auth" }).click();
    await page.getByLabel("Auth type").selectOption("bearer");
    await page.getByLabel("Token", { exact: true }).fill("tok-123");
    await page.getByRole("button", { name: "Save as secret variable…" }).click();
    await page.getByRole("dialog", { name: "Save as secret variable" }).getByRole("button", { name: "Save" }).click();
    await expect(page.getByLabel("Token", { exact: true })).toHaveValue("{{token}}");
  });
});
