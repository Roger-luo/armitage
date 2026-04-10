# Roadmap Chart Design Spec

Interactive HTML roadmap visualization for armitage. Generates a standalone HTML page from the node hierarchy with drill-down navigation through initiative/project/task levels.

## Goals

1. **Visual overview** — see the full roadmap timeline at a glance, with nested bars showing work distribution within each initiative.
2. **Drill-down navigation** — click an initiative to see its projects, click a project to see its tasks, and so on. Breadcrumb navigation to go back.
3. **Single-file output** — one `.html` file, no server, opens in any browser.
4. **Milestone visibility** — milestones and OKRs render as vertical dashed lines on the time axis with hoverable labels.

## Non-Goals

- Real-time updates or live data connections
- Editing nodes from the chart UI
- Server-side rendering or hosting

## Architecture

### Stack

- **Rust (askama)** — walks the node tree, builds chart data, renders HTML template with embedded JSON and JS
- **TypeScript → JS** — chart logic (ECharts setup, renderItem, drill-down, breadcrumb) written in TS, compiled to JS via esbuild, checked into the repo
- **Apache ECharts** — rendering engine. Custom series with `renderItem` for the nested bar layout. Loaded from CDN by default, inlined for offline mode.

### Crate: `armitage-chart`

```
crates/armitage-chart/
  Cargo.toml
  ts/
    chart.ts               # ECharts setup, renderItem, drill-down, breadcrumb
    types.ts               # ChartNode, ChartData interfaces (mirror Rust types)
    tsconfig.json
  js/
    chart.js               # compiled output, checked into git
  scripts/
    build-js.sh            # esbuild ts/chart.ts → js/chart.js
  templates/
    chart.html             # askama template
  src/
    lib.rs                 # public API: build_chart_data(), render_chart()
    error.rs               # Error + Result (thiserror)
    data.rs                # ChartNode, ChartMilestone, ChartData, build logic
    render.rs              # askama template struct, render_chart()
```

**Dependencies:** `armitage-core`, `armitage-milestones`, `serde`, `serde_json`, `chrono`, `thiserror`, `askama`

**Dependency position:** Layer 1 — depends only on `armitage-core` and `armitage-milestones`.

### JS Build

TypeScript source in `ts/`, compiled JS checked into `js/`. Regenerate via:

```bash
./scripts/build-js.sh   # esbuild ts/chart.ts --bundle --outfile=js/chart.js --format=iife
```

`cargo build` uses the checked-in JS via `include_str!` in the askama template. No Node.js required for Rust builds. CI can optionally verify the checked-in JS matches the TS source.

## Data Model

### Rust Types (`data.rs`)

```rust
#[derive(Serialize)]
pub struct ChartNode {
    pub path: String,              // "stela/auth-service"
    pub name: String,
    pub description: String,
    pub status: String,            // "active"/"completed"/"paused"/"cancelled"
    pub start: Option<String>,     // ISO date from node's own timeline
    pub end: Option<String>,
    pub eff_start: Option<String>, // effective: own timeline OR min(children starts)
    pub eff_end: Option<String>,   // effective: own timeline OR max(children ends)
    pub has_timeline: bool,        // false → gray dashed bar
    pub owners: Vec<String>,
    pub team: Option<String>,
    pub children: Vec<ChartNode>,
    pub milestones: Vec<ChartMilestone>,
}

#[derive(Serialize)]
pub struct ChartMilestone {
    pub name: String,
    pub date: String,              // ISO date
    pub description: String,
    pub milestone_type: String,    // "checkpoint" or "okr"
}

#[derive(Serialize)]
pub struct ChartData {
    pub nodes: Vec<ChartNode>,     // top-level initiatives
    pub org_name: String,
    pub global_start: Option<String>,
    pub global_end: Option<String>,
}
```

### `build_chart_data(entries: &[NodeEntry], org_root: &Path, org_name: &str) -> Result<ChartData>`

1. Build `HashMap<String, Vec<&NodeEntry>>` grouping entries by parent path (via `path.rfind('/')`)
2. Recursively construct `ChartNode` tree from root-level entries down
3. For each node, read `milestones.toml` from its directory if present
4. Compute effective timelines bottom-up: if a node has no timeline, derive `eff_start`/`eff_end` from `min(children.eff_start)` and `max(children.eff_end)`
5. Compute `global_start`/`global_end` as the min/max across all effective timelines

### TypeScript Types (`types.ts`)

Mirror the Rust types as TS interfaces. `ChartData` is injected into the page as `window.__CHART_DATA__`.

## Visualization Design

### Top-Level View (Initiatives)

Each initiative is one row. The row contains:

- **Outer bar:** Spans `eff_start` to `eff_end`.
  - Node has own timeline → solid border, colored fill (by status)
  - Node has no timeline → dashed border, gray fill
- **Nested child bars:** First-level subprojects rendered as thin horizontal bars stacked inside the outer bar. These use the child's own status color regardless of the parent's color. No labels on nested bars — just visual density showing where work is concentrated.
- **Label:** Initiative name on the Y-axis only.

### Drill-Down View

Clicking an initiative replaces the entire view with its children as full rows. Each child row follows the same pattern: outer bar with its own nested children inside. The breadcrumb updates to "Root > Initiative Name".

This pattern is recursive: each level shows the current level's timeline bars with the next level nested inside.

### Status Colors

| Status    | Color   | Use                          |
|-----------|---------|------------------------------|
| Active    | #3b82f6 | Blue, solid fill             |
| Completed | #6b7280 | Gray, solid fill             |
| Paused    | #f59e0b | Amber, solid fill            |
| Cancelled | #ef4444 | Red, solid fill              |
| No timeline | —    | Gray dashed border, light gray fill. Child bars inside retain their own status colors. |

### Milestones & OKRs

Rendered as vertical dashed lines on the time axis (not on the bars). Each line has a small label at the top of the chart with the milestone name. Hovering the label shows a tooltip with: name, date, type (checkpoint/OKR), and description.

Milestones shown are those belonging to nodes visible at the current drill-down level.

### Time Axis

- Default: auto-rescaled to fit the visible nodes' effective timelines, with 30-day padding on each side.
- Toggle button switches to global time range (min/max across entire org).
- Quarter markers shown as subtle vertical gridlines.

### Tooltip (on bar hover)

Shows: name, timeline dates (or "No fixed timeline"), status, owners, team.

### Navigation

Breadcrumb bar at the top: "Root > Stela > Auth" — each segment is a clickable link. Clicking "Root" returns to the top-level initiative view. Clicking any intermediate segment navigates to that level.

## CLI Integration

### Command

```
armitage chart [--output PATH] [--no-open] [--offline]
```

| Flag       | Default                    | Description                              |
|------------|----------------------------|------------------------------------------|
| `--output` | `.armitage/chart.html`     | Output file path                         |
| `--no-open`| false                      | Skip auto-opening browser                |
| `--offline`| false                      | Inline ECharts JS (no CDN)               |

### Handler (`crates/armitage/src/cli/chart.rs`)

1. Find org root, open org, walk nodes
2. Call `build_chart_data()` to construct the hierarchy with milestones
3. Call `render_chart()` to produce HTML string
4. Write to output path (ensure parent dir exists)
5. Open in browser unless `--no-open` (platform detection: `open` on macOS, `xdg-open` on Linux)

### Files Modified

- `Cargo.toml` — add `"crates/armitage-chart"` to workspace members; add `armitage-chart` and `askama` to `[workspace.dependencies]`
- `crates/armitage/Cargo.toml` — add `armitage-chart = { workspace = true }`
- `crates/armitage/src/error.rs` — add `Chart(#[from] armitage_chart::error::Error)` variant
- `crates/armitage/src/cli/mod.rs` — add `pub mod chart;`, `Chart` variant to `Commands` enum, dispatch in `run()`
- `SKILL.md` — document `armitage chart` command

## Testing

### Unit Tests (`armitage-chart`)

- **`data.rs`:** Build chart data from a temp org with known hierarchy. Assert tree structure, effective timeline computation, milestone inclusion, status mapping.
- **`render.rs`:** Render sample ChartData to HTML. Assert output contains DOCTYPE, ECharts script, org name, embedded JSON data.

### Integration Test (`crates/armitage/tests/`)

- Set up temp org with nodes and milestones, call `run_chart` with `--no-open`, verify HTML file is written and contains expected content.

### Manual Verification

Build and run against the test org:
```bash
cargo build
cd <test-org>
cargo run -- chart --no-open
# Open .armitage/chart.html in browser, verify drill-down works
```
