import { test, expect } from "@playwright/test";
import { fixtureUrl } from "../helpers/generate-html";

test.describe("basic rendering", () => {
  test("root view renders all top-level nodes", async ({ page }) => {
    await page.goto(fixtureUrl("basic"));
    await page.waitForSelector("#chart canvas", { timeout: 5000 });
    await page.waitForTimeout(500);
    await expect(page).toHaveScreenshot("basic-root.png");
  });

  test("breadcrumb shows org name at root", async ({ page }) => {
    await page.goto(fixtureUrl("basic"));
    await page.waitForSelector("#chart canvas", { timeout: 5000 });
    const text = await page.textContent("#breadcrumb");
    expect(text).toContain("TestOrg");
  });

  test("edge cases render without errors", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (err) => errors.push(err.message));
    await page.goto(fixtureUrl("edge-cases"));
    await page.waitForSelector("#chart canvas", { timeout: 5000 });
    await page.waitForTimeout(500);
    expect(errors).toEqual([]);
    await expect(page).toHaveScreenshot("edge-cases-root.png");
  });

  test("with-issues fixture renders without errors", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (err) => errors.push(err.message));
    await page.goto(fixtureUrl("with-issues"));
    await page.waitForSelector("#chart canvas", { timeout: 5000 });
    await page.waitForTimeout(500);
    expect(errors).toEqual([]);
  });
});
