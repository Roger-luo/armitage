import { defineConfig } from "@playwright/test";
import path from "path";

export default defineConfig({
  testDir: "./specs",
  outputDir: "../test-results",
  snapshotDir: "./screenshots",
  globalSetup: "./helpers/setup.ts",
  timeout: 15000,
  expect: {
    toHaveScreenshot: {
      maxDiffPixelRatio: 0.01,
    },
  },
  use: {
    viewport: { width: 1400, height: 900 },
    colorScheme: "dark",
  },
  projects: [
    {
      name: "chromium",
      use: {
        browserName: "chromium",
      },
    },
  ],
});
