import fs from "fs";
import path from "path";

const CRATE_ROOT = path.resolve(__dirname, "../..");
const CHART_JS_PATH = path.join(CRATE_ROOT, "js/chart.js");
const TEMPLATE_PATH = path.join(CRATE_ROOT, "templates/chart.html");

/**
 * Extract the <style>...</style> block from the Askama template.
 * This keeps the standalone test HTML visually consistent with the real template.
 */
function extractCss(): string {
  const template = fs.readFileSync(TEMPLATE_PATH, "utf-8");
  const match = template.match(/<style>([\s\S]*?)<\/style>/);
  return match ? match[1] : "";
}

/**
 * Generate a self-contained chart HTML file from a ChartData JSON fixture.
 *
 * @param fixtureJsonPath - path to a ChartData JSON file
 * @param outputHtmlPath - where to write the resulting HTML
 */
export function generateHtml(
  fixtureJsonPath: string,
  outputHtmlPath: string,
): void {
  const chartJs = fs.readFileSync(CHART_JS_PATH, "utf-8");
  const fixtureData = fs.readFileSync(fixtureJsonPath, "utf-8");
  const css = extractCss();

  const html = `<!DOCTYPE html>
<html lang="en" data-theme="dark">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Test Chart</title>
  <script src="https://cdn.jsdelivr.net/npm/d3@7/dist/d3.min.js"></script>
  <style>${css}</style>
</head>
<body>
  <div id="nav">
    <div id="breadcrumb"></div>
    <div class="seg-toggle">
      <button id="btn-fitted" class="active" onclick="window.__setRange(false)">Fitted</button>
      <button id="btn-global" onclick="window.__setRange(true)">Global</button>
    </div>
    <div class="seg-toggle" id="theme-toggle">
      <button onclick="window.__setTheme('light')">☀</button>
      <button class="active" onclick="window.__setTheme('dark')">☾</button>
      <button onclick="window.__setTheme('auto')">Auto</button>
    </div>
  </div>
  <div class="chart-tooltip" id="tooltip" style="display:none"></div>
  <div id="main">
    <div id="chart-scroll">
      <div id="chart-labels"></div>
      <div id="chart-timeline">
        <svg id="chart-svg">
          <defs>
            <linearGradient id="heat-gradient" x1="0" y1="0" x2="1" y2="0">
              <stop offset="0%" stop-color="rgba(88,166,255,0.35)"/>
              <stop offset="100%" stop-color="rgba(88,166,255,0.15)"/>
            </linearGradient>
          </defs>
          <g id="axis-group"></g>
          <g id="grid-group"></g>
          <g id="bars-group"></g>
          <g id="markers-group"></g>
        </svg>
      </div>
    </div>
    <div id="panel">
      <button id="panel-close" onclick="window.__closePanel()">&times;</button>
      <div id="panel-content"></div>
    </div>
  </div>

  <script>
    window.__setTheme = function(mode) {
      var html = document.documentElement;
      var btns = document.querySelectorAll('#theme-toggle button');
      btns.forEach(function(b) { b.classList.remove('active'); });
      if (mode === 'auto') {
        var prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
        html.setAttribute('data-theme', prefersDark ? 'dark' : 'light');
        btns[2].classList.add('active');
      } else {
        html.setAttribute('data-theme', mode);
        btns[mode === 'light' ? 0 : 1].classList.add('active');
      }
      localStorage.setItem('armitage-chart-theme', mode);
      if (window.__onThemeChange) window.__onThemeChange();
    };
    window.__setTheme('dark');
  </script>
  <script>
    window.__CHART_DATA__ = ${fixtureData};
    ${chartJs}
  </script>
</body>
</html>`;

  fs.mkdirSync(path.dirname(outputHtmlPath), { recursive: true });
  fs.writeFileSync(outputHtmlPath, html);
}

/**
 * Return the output directory for generated HTML files.
 */
export function htmlOutputDir(): string {
  return path.join(CRATE_ROOT, "test-results/html");
}

/**
 * Get the file:// URL for a named fixture's generated HTML.
 */
export function fixtureUrl(name: string): string {
  return `file://${path.join(htmlOutputDir(), `${name}.html`)}`;
}
