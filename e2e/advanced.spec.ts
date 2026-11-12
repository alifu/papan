import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "./tauriMock";

const CURL = `curl --data-raw '{"x":1}' https://api.example.com/items`;

async function openScratch(page: Page) {
  await page.getByRole("button", { name: "Scratch" }).click();
  await page.getByRole("button", { name: /Quick check/ }).click();
  await expect(page.getByRole("tab", { name: "Quick check" })).toBeVisible();
}

test.describe("advanced http", () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => {
      Object.defineProperty(navigator, "clipboard", { value: { writeText: () => Promise.resolve() }, configurable: true });
    });
    await mockTauri(page);
    await page.goto("/");
    await openScratch(page);
  });

  test("imports a pasted cURL command after confirmation and copies as cURL", async ({ page }) => {
    await page.getByLabel("URL").click();
    await page.getByLabel("URL").evaluate((el, text) => {
      const data = new DataTransfer();
      data.setData("text", text);
      el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
    }, CURL);
    await expect(page.getByText("Replace this request with the pasted cURL command?")).toBeVisible();
    await page.getByRole("button", { name: "Replace" }).click();

    await expect(page.getByLabel("URL")).toHaveText("https://api.example.com/items");
    await expect(page.getByLabel("Method")).toHaveValue("POST");
    await page.getByRole("tab", { name: /Headers/ }).click();
    await expect(page.getByLabel("Name 1")).toHaveValue("Content-Type");

    await page.keyboard.press("ControlOrMeta+Shift+C");
    await expect(page.getByText("Copied. Secret values are left as {{name}}.")).toBeVisible();
  });

  test("imports a cURL command from the File menu into a new request", async ({ page }) => {
    await page.getByRole("menuitem", { name: "File" }).click();
    await page.getByRole("menuitem", { name: "Import cURL…" }).click();
    const dialog = page.getByRole("dialog", { name: "Import cURL" });
    await dialog.getByLabel("cURL command").fill(CURL);
    await dialog.getByRole("button", { name: "Create request" }).click();
    await expect(dialog).toBeHidden();
    await expect(page.getByLabel("URL")).toHaveText("https://api.example.com/items");
  });

  test("keeps the proxy password write-only and shows the Proxy badge", async ({ page }) => {
    await page.getByRole("menuitem", { name: "Edit" }).click();
    await page.getByRole("menuitem", { name: "Network settings…" }).click();
    const dialog = page.getByRole("dialog", { name: "Network settings" });
    await dialog.getByLabel("Custom proxy").check();
    await dialog.getByLabel("Proxy URL").fill("http://proxy.local:8080");
    await dialog.getByLabel("Username").fill("alice");
    await dialog.getByLabel("Password").fill("pr0xy-secret");
    await dialog.getByRole("button", { name: "Save proxy" }).click();
    await dialog.getByRole("button", { name: "Cancel" }).click();

    await expect(page.locator(".proxy-badge")).toBeVisible();
    await page.getByRole("menuitem", { name: "Edit" }).click();
    await page.getByRole("menuitem", { name: "Network settings…" }).click();
    const password = page.getByRole("dialog", { name: "Network settings" }).getByLabel("Password");
    await expect(password).toHaveValue("");
    await expect(password).toHaveAttribute("placeholder", "Stored");
    expect(await page.content()).not.toContain("pr0xy-secret");
  });

  test("shows the timing phases and manages cookies", async ({ page }) => {
    await page.getByLabel("URL").fill("http://localhost/json");
    await page.getByRole("button", { name: "Send" }).click();
    await expect(page.getByText("200 OK")).toBeVisible();

    await page.getByRole("button", { name: "12 ms" }).click();
    const timing = page.getByRole("dialog", { name: "Timing" });
    for (const label of ["DNS", "Connect + TLS", "Waiting", "Download"]) {
      await expect(timing.getByText(label, { exact: true })).toBeVisible();
    }
    await expect(timing.getByText("7 ms")).toBeVisible();

    await page.keyboard.press("Escape");
    await expect(timing).toBeHidden();

    await page.getByRole("tab", { name: /Cookies/ }).click();
    await page.getByRole("button", { name: "Manage cookies…" }).click();
    const jar = page.getByRole("dialog", { name: "Cookies" });
    await jar.getByRole("button", { name: "Add cookie…" }).click();
    const form = page.getByRole("dialog", { name: "Add cookie" });
    await form.getByLabel("Name", { exact: true }).fill("sid");
    await form.getByLabel("Value").fill("abc");
    await form.getByLabel("Domain").fill("localhost");
    await form.getByRole("button", { name: "Save" }).click();
    await expect(jar.getByRole("row", { name: /sid/ })).toBeVisible();

    await jar.getByRole("button", { name: "Delete cookie sid" }).click();
    await expect(jar.getByText("No cookies.")).toBeVisible();
  });
});
