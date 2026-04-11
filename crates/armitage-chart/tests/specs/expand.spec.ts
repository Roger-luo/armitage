import { test, expect } from "@playwright/test";
import { fixtureUrl } from "../helpers/generate-html";

test.describe("expand/collapse issues", () => {
  test("clicking leaf node with issues expands issue rows", async ({
    page,
  }) => {
    await page.goto(fixtureUrl("with-issues"));
    await page.waitForSelector("#chart-svg", { timeout: 5000 });

    // Drill into "signal" to see leaf nodes (Filters and Transforms)
    await page.evaluate(() => (window as any).__nav("signal"));
    await page.waitForTimeout(500);

    // Click on the first node row (Filters)
    await page.click(".chart-row.node", { timeout: 2000 });
    await page.waitForTimeout(800);

    await expect(page).toHaveScreenshot("issues-expanded.png");
  });

  test("expanded view shows overdue issues with warning prefix", async ({
    page,
  }) => {
    await page.goto(fixtureUrl("with-issues"));
    await page.waitForSelector("#chart-svg", { timeout: 5000 });

    await page.evaluate(() => (window as any).__nav("signal"));
    await page.waitForTimeout(500);

    // Click on the first node row (Filters) to expand issues
    await page.click(".chart-row.node", { timeout: 2000 });
    await page.waitForTimeout(800);

    // Check DOM for overdue issue labels
    const overdueLabels = await page.$$eval(
      ".chart-label.issue-title.overdue",
      (els) => els.map((el) => el.textContent),
    );
    expect(overdueLabels.length).toBeGreaterThan(0);

    // Check for separator rows
    const separators = await page.$$eval(
      ".chart-row.separator",
      (els) => els.length,
    );
    expect(separators).toBeGreaterThan(0);
  });

  test("clicking expanded bar again collapses issue rows", async ({
    page,
  }) => {
    await page.goto(fixtureUrl("with-issues"));
    await page.waitForSelector("#chart-svg", { timeout: 5000 });

    await page.evaluate(() => (window as any).__nav("signal"));
    await page.waitForTimeout(500);

    // Expand by clicking the first node row (Filters)
    await page.click(".chart-row.node", { timeout: 2000 });
    await page.waitForTimeout(500);

    // Count rows while expanded
    const expandedCount = await page.$$eval(
      ".chart-row",
      (els) => els.length,
    );

    // Collapse by clicking the same node row again
    await page.click(".chart-row.node", { timeout: 2000 });
    await page.waitForTimeout(500);

    const collapsedCount = await page.$$eval(
      ".chart-row",
      (els) => els.length,
    );

    expect(collapsedCount).toBeLessThan(expandedCount);
  });
});
