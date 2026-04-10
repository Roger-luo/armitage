import { test, expect } from "@playwright/test";
import { fixtureUrl } from "../helpers/generate-html";

test.describe("expand/collapse issues", () => {
  test("clicking leaf node with issues expands issue rows", async ({
    page,
  }) => {
    await page.goto(fixtureUrl("with-issues"));
    await page.waitForSelector("#chart canvas", { timeout: 5000 });

    // Drill into "signal" to see leaf nodes (Filters and Transforms)
    await page.evaluate(() => (window as any).__nav("signal"));
    await page.waitForTimeout(500);

    // Get chart bounding box
    const chart = page.locator("#chart");
    const box = await chart.boundingBox();
    expect(box).not.toBeNull();

    // Click on the Filters bar (bottom row in the chart, ECharts renders categories bottom-to-top)
    await page.mouse.click(box!.x + box!.width * 0.4, box!.y + 200);
    await page.waitForTimeout(800);

    await expect(page).toHaveScreenshot("issues-expanded.png");
  });

  test("expanded view shows overdue issues with warning prefix", async ({
    page,
  }) => {
    await page.goto(fixtureUrl("with-issues"));
    await page.waitForSelector("#chart canvas", { timeout: 5000 });

    await page.evaluate(() => (window as any).__nav("signal"));
    await page.waitForTimeout(500);

    const chart = page.locator("#chart");
    const box = await chart.boundingBox();
    // Click Filters bar (rendered near bottom in ECharts horizontal bar chart)
    await page.mouse.click(box!.x + box!.width * 0.4, box!.y + 200);
    await page.waitForTimeout(800);

    // Check Y-axis labels for overdue indicators
    const categories = await page.evaluate(() => {
      const chartDom = document.getElementById("chart");
      const instance = (window as any).echarts.getInstanceByDom(chartDom);
      const option = instance.getOption();
      return option.yAxis[0].data as string[];
    });

    // Should contain overdue issues with ⚠ prefix
    const overdueLabels = categories.filter((c: string) => c.startsWith("⚠"));
    expect(overdueLabels.length).toBeGreaterThan(0);

    // Should contain separator
    const separators = categories.filter((c: string) => c === "───");
    expect(separators.length).toBeGreaterThan(0);
  });

  test("clicking expanded bar again collapses issue rows", async ({
    page,
  }) => {
    await page.goto(fixtureUrl("with-issues"));
    await page.waitForSelector("#chart canvas", { timeout: 5000 });

    await page.evaluate(() => (window as any).__nav("signal"));
    await page.waitForTimeout(500);

    const chart = page.locator("#chart");
    const box = await chart.boundingBox();

    // Expand by clicking the Filters bar
    await page.mouse.click(box!.x + box!.width * 0.4, box!.y + 200);
    await page.waitForTimeout(500);

    // Get category count while expanded
    const expandedCount = await page.evaluate(() => {
      const chartDom = document.getElementById("chart");
      const instance = (window as any).echarts.getInstanceByDom(chartDom);
      return (instance.getOption().yAxis[0].data as string[]).length;
    });

    // Collapse — Filters bar shifts down after issue rows are inserted above it
    await page.mouse.click(box!.x + box!.width * 0.4, box!.y + 700);
    await page.waitForTimeout(500);

    const collapsedCount = await page.evaluate(() => {
      const chartDom = document.getElementById("chart");
      const instance = (window as any).echarts.getInstanceByDom(chartDom);
      return (instance.getOption().yAxis[0].data as string[]).length;
    });

    expect(collapsedCount).toBeLessThan(expandedCount);
  });
});
