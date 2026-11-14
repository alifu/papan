import { expect, test, type Page } from "@playwright/test";
import { installSoapFake } from "./soapMock";
import { mockTauri } from "./tauriMock";

async function newSoapScratch(page: Page) {
  await page.getByRole("button", { name: "Scratch" }).click();
  await page.getByRole("menuitem", { name: "New Request" }).click();
  await page.getByRole("menuitem", { name: "SOAP" }).click();
  await expect(page.getByRole("tab", { name: "Untitled" })).toBeVisible();
  await page.getByRole("tab", { name: "SOAP", exact: true }).click();
}

async function importFromUrl(page: Page, url: string) {
  await page.getByRole("button", { name: "Import WSDL…" }).click();
  const dialog = page.getByRole("dialog", { name: "Import WSDL" });
  await dialog.getByLabel("WSDL URL").fill(url);
  await dialog.getByRole("button", { name: "Import", exact: true }).click();
  return dialog;
}

test.describe("soap", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await installSoapFake(page);
    await page.goto("/");
    await newSoapScratch(page);
  });

  test("imports a WSDL, lists warnings and generates an envelope into the body", async ({ page }) => {
    const dialog = await importFromUrl(page, "http://localhost/calc?wsdl");
    await expect(dialog.getByLabel("Warnings")).toContainText("Skipped the HTTP binding");
    await expect(dialog.getByLabel("Fetched documents")).toContainText("calc.xsd");
    await dialog.getByRole("button", { name: "Use this WSDL" }).click();

    await expect(page.locator(".soap-badge")).toHaveText("SOAP 1.2");
    await page.getByRole("button", { name: "Add", exact: true }).click();

    await expect(page.getByLabel("URL")).toContainText("http://localhost/calc");
    await expect(page.getByLabel("SOAPAction")).toContainText("http://tempuri.org/Add");
    await page.getByRole("tab", { name: /Body/ }).click();
    await expect(page.getByText("ns1:Add", { exact: false }).first()).toBeVisible();
  });

  test("drops optional elements when the switch is off", async ({ page }) => {
    const dialog = await importFromUrl(page, "http://localhost/calc?wsdl");
    await dialog.getByRole("button", { name: "Use this WSDL" }).click();
    await page.getByRole("checkbox", { name: "Include optional elements" }).uncheck();
    await page.getByRole("button", { name: "Subtract", exact: true }).click();

    await page.getByRole("tab", { name: /Body/ }).click();
    await expect(page.getByText("ns1:Subtract", { exact: false }).first()).toBeVisible();
    await expect(page.getByText("ns1:note")).toHaveCount(0);
  });

  test("shows the backend message when the import fails", async ({ page }) => {
    const dialog = await importFromUrl(page, "http://localhost/bad?wsdl");
    await expect(dialog.getByRole("alert")).toContainText("WSDL 2.0 is not supported");
  });
});
