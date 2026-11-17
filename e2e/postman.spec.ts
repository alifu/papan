import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "./tauriMock";

const calls = (page: Page, cmd: string) =>
  page.evaluate((c) => window.__postmanCalls.filter((x) => x.cmd === c).map((x) => x.args), cmd);

async function openFileMenu(page: Page, item: string) {
  await page.getByRole("menuitem", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: item }).click();
}

test.describe("postman", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await page.goto("/");
    await expect(page.getByRole("treeitem", { name: "Demo API", exact: true })).toBeVisible();
  });

  test("import: preview, untick a secret, import, report, collection appears", async ({ page }) => {
    await openFileMenu(page, "Import Postman…");
    await page.getByRole("button", { name: "Choose files…" }).click();

    const dialog = page.getByRole("dialog", { name: "Import Postman" });
    await expect(dialog.getByText(/3 requests, 1 folders/)).toBeVisible();
    await expect(dialog.getByText(/switched off/).first()).toBeVisible();
    await expect(dialog.getByText("pm.cookies is not supported")).toBeVisible();
    await expect(dialog.getByLabel("Store secret values on this machine")).not.toBeChecked();
    await dialog.getByLabel("apiKey").uncheck();
    await dialog.getByRole("button", { name: "Import", exact: true }).click();

    await expect(dialog.getByText(/2 scripts imported but switched off/)).toBeVisible();
    expect(await calls(page, "postman_import_commit")).toEqual([
      {
        token: "tok-1",
        options: {
          target: { type: "new_collection" },
          secret_names: ["token"],
          store_secret_values: false,
          import_scripts: true,
          globals_clash: "keep",
        },
      },
    ]);
    await dialog.getByRole("button", { name: "Close" }).click();
    await expect(page.getByRole("treeitem", { name: "Shop", exact: true })).toBeVisible();
    expect(await calls(page, "postman_import_discard")).toEqual([]);
  });

  test("import: cancelling the file dialog is not an error, cancelling the preview discards the stage", async ({ page }) => {
    await openFileMenu(page, "Import Postman…");
    await page.evaluate(() => window.__postmanCancelNext());
    await page.getByRole("button", { name: "Choose files…" }).click();
    await expect(page.getByRole("button", { name: "Choose files…" })).toBeEnabled();
    await expect(page.getByRole("alert")).toHaveCount(0);

    await page.getByRole("button", { name: "Choose files…" }).click();
    await expect(page.getByRole("list", { name: "Files" })).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    expect(await calls(page, "postman_import_discard")).toEqual([{ token: "tok-1" }]);
  });

  test("export from the row menu: prefilled, warns on secrets, shows written files and report", async ({ page }) => {
    const row = page.getByRole("treeitem", { name: "Demo API", exact: true });
    await row.hover();
    await page.getByRole("menuitem", { name: "Actions for Demo API", exact: true }).click();
    await page.getByRole("menuitem", { name: "Export to Postman…" }).click();

    const dialog = page.getByRole("dialog", { name: "Export to Postman" });
    await expect(dialog.getByLabel("Collection")).toHaveValue("c1");
    await expect(dialog.getByLabel("Include secret values")).not.toBeChecked();
    await expect(dialog.getByRole("alert")).toHaveCount(0);
    await dialog.getByLabel("Include secret values").check();
    await expect(dialog.getByRole("alert")).toContainText("plain text");
    await dialog.getByLabel("Include secret values").uncheck();
    await dialog.getByRole("button", { name: "Export", exact: true }).click();

    await expect(dialog.getByText("demo-api.postman_collection.json")).toBeVisible();
    await expect(dialog.getByText("WebSocket requests are not exported")).toBeVisible();
    expect(await calls(page, "postman_export")).toEqual([
      { options: { collection_id: "c1", environments: [], include_globals: false, include_secret_values: false } },
    ]);
  });

  test("export from the File menu; cancelling the folder dialog keeps the dialog open", async ({ page }) => {
    await openFileMenu(page, "Export to Postman…");
    const dialog = page.getByRole("dialog", { name: "Export to Postman" });
    await page.evaluate(() => window.__postmanCancelNext());
    await dialog.getByRole("button", { name: "Export", exact: true }).click();
    await expect(dialog.getByRole("button", { name: "Export", exact: true })).toBeEnabled();
    await expect(page.getByRole("alert")).toHaveCount(0);
    await dialog.getByRole("button", { name: "Export", exact: true }).click();
    await expect(dialog.getByRole("list", { name: "Written files" })).toBeVisible();
  });
});
