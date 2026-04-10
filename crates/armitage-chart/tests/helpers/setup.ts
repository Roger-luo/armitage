import fs from "fs";
import path from "path";
import { generateHtml, htmlOutputDir } from "./generate-html";

const FIXTURES_DIR = path.resolve(__dirname, "../fixtures");

async function globalSetup() {
  const mode = process.env.CHART_TEST_MODE || "standalone";

  if (mode === "rust") {
    const dir = htmlOutputDir();
    if (!fs.existsSync(dir)) {
      throw new Error(
        `CHART_TEST_MODE=rust but no HTML files found at ${dir}. ` +
          `Run 'cargo test -p armitage-chart generate_test_html' first.`,
      );
    }
    return;
  }

  // Standalone mode: generate HTML from fixture JSON files
  const fixtureFiles = fs
    .readdirSync(FIXTURES_DIR)
    .filter((f) => f.endsWith(".json"));

  for (const file of fixtureFiles) {
    const name = path.basename(file, ".json");
    const fixturePath = path.join(FIXTURES_DIR, file);
    const outputPath = path.join(htmlOutputDir(), `${name}.html`);
    generateHtml(fixturePath, outputPath);
  }

  console.log(
    `Generated ${fixtureFiles.length} test HTML files in ${htmlOutputDir()}`,
  );
}

export default globalSetup;
