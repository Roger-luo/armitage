import { test, expect } from "@playwright/test";
import { fixtureUrl } from "../helpers/generate-html";

test.describe("navigation", () => {
  test("drill into node via __nav shows children", async ({ page }) => {
    await page.goto(fixtureUrl("deep-tree"));
    await page.waitForSelector("#chart canvas", { timeout: 5000 });
    await page.waitForTimeout(500);

    await page.evaluate(() => (window as any).__nav("infra"));
    await page.waitForTimeout(500);

    const breadcrumb = await page.textContent("#breadcrumb");
    expect(breadcrumb).toContain("Infrastructure");
    expect(breadcrumb).toContain("›");

    await expect(page).toHaveScreenshot("drilled-into-infra.png");
  });

  test("drill two levels deep shows correct breadcrumb", async ({ page }) => {
    await page.goto(fixtureUrl("deep-tree"));
    await page.waitForSelector("#chart canvas", { timeout: 5000 });

    await page.evaluate(() => (window as any).__nav("infra/networking"));
    await page.waitForTimeout(500);

    const breadcrumb = await page.textContent("#breadcrumb");
    expect(breadcrumb).toContain("Networking");

    await expect(page).toHaveScreenshot("drilled-two-levels.png");
  });

  test("navigate back to root via breadcrumb", async ({ page }) => {
    await page.goto(fixtureUrl("deep-tree"));
    await page.waitForSelector("#chart canvas", { timeout: 5000 });

    await page.evaluate(() => (window as any).__nav("infra"));
    await page.waitForTimeout(300);

    await page.evaluate(() => (window as any).__nav(""));
    await page.waitForTimeout(500);

    const breadcrumb = await page.textContent("#breadcrumb");
    expect(breadcrumb).not.toContain("›");

    await expect(page).toHaveScreenshot("navigated-back-to-root.png");
  });
});
