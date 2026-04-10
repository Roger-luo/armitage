import type { ChartData, ChartNode, ChartMilestone, ChartIssue } from "./types";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const data: ChartData = window.__CHART_DATA__;
let currentPath = ""; // "" = root
let useGlobalRange = false;
let selectedNode: ChartNode | null = null;

// Current visible nodes — shared between buildOption() and renderBar()
let visibleNodes: ChartNode[] = [];

// ---------------------------------------------------------------------------
// DOM references
// ---------------------------------------------------------------------------

const chartEl = document.getElementById("chart")!;
const breadcrumbEl = document.getElementById("breadcrumb")!;
const toggleBtn = document.getElementById("toggle-range")!;
const panelEl = document.getElementById("panel")!;
const panelContentEl = document.getElementById("panel-content")!;
const chart = echarts.init(chartEl);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const STATUS_COLORS: Record<string, string> = {
  active: "#3b82f6",
  completed: "#6b7280",
  paused: "#f59e0b",
  cancelled: "#ef4444",
};

const NO_TIMELINE_COLOR = "rgba(107, 114, 128, 0.15)";
const NO_TIMELINE_BORDER = "rgba(107, 114, 128, 0.4)";

function parseDate(s: string): number {
  return new Date(s + "T00:00:00").getTime();
}

/** Find nodes to display at the current navigation level. */
function getVisibleNodes(): ChartNode[] {
  if (currentPath === "") return data.nodes;
  const node = findNode(data.nodes, currentPath);
  return node ? node.children : [];
}

/** Recursively find a node by path. */
function findNode(nodes: ChartNode[], path: string): ChartNode | null {
  for (const n of nodes) {
    if (n.path === path) return n;
    const found = findNode(n.children, path);
    if (found) return found;
  }
  return null;
}

/** Collect checkpoints only (not OKRs) from a node and descendants. */
function allCheckpoints(node: ChartNode): ChartMilestone[] {
  const result: ChartMilestone[] = [];
  function walk(n: ChartNode) {
    for (const m of n.milestones) {
      if (m.milestone_type !== "okr") result.push(m);
    }
    for (const c of n.children) walk(c);
  }
  walk(node);
  return result;
}

/** Collect all OKRs across all nodes in the entire tree (org-wide). */
function collectOkrs(nodes: ChartNode[]): ChartMilestone[] {
  const seen = new Set<string>();
  const result: ChartMilestone[] = [];
  function walk(ns: ChartNode[]) {
    for (const n of ns) {
      for (const m of n.milestones) {
        if (m.milestone_type === "okr") {
          const key = `${m.name}|${m.date}`;
          if (!seen.has(key)) {
            seen.add(key);
            result.push(m);
          }
        }
      }
      walk(n.children);
    }
  }
  walk(nodes);
  return result;
}

/** Compute time range for visible nodes. */
function computeTimeRange(nodes: ChartNode[]): [number, number] {
  if (useGlobalRange && data.global_start && data.global_end) {
    return [parseDate(data.global_start), parseDate(data.global_end)];
  }

  let min = Infinity;
  let max = -Infinity;
  for (const n of nodes) {
    const s = n.eff_start;
    const e = n.eff_end;
    if (s) min = Math.min(min, parseDate(s));
    if (e) max = Math.max(max, parseDate(e));
  }

  if (min === Infinity || max === -Infinity) {
    const now = new Date();
    min = new Date(now.getFullYear(), 0, 1).getTime();
    max = new Date(now.getFullYear(), 11, 31).getTime();
  }

  const pad = 30 * 24 * 3600 * 1000;
  return [min - pad, max + pad];
}

// ---------------------------------------------------------------------------
// Issue helpers
// ---------------------------------------------------------------------------

interface SortedIssues {
  overdue: ChartIssue[];
  onTrack: ChartIssue[];
  noDates: ChartIssue[];
}

/** Sort issues into overdue / on-track / no-dates buckets. */
function sortIssues(issues: ChartIssue[], nodeEnd: string | null): SortedIssues {
  const overdue: ChartIssue[] = [];
  const onTrack: ChartIssue[] = [];
  const noDates: ChartIssue[] = [];

  for (const issue of issues) {
    if (!issue.target_date) {
      noDates.push(issue);
    } else if (nodeEnd && issue.target_date > nodeEnd) {
      overdue.push(issue);
    } else {
      onTrack.push(issue);
    }
  }

  // Overdue: most overdue first (latest target_date first, since all exceed nodeEnd)
  overdue.sort((a, b) => b.target_date!.localeCompare(a.target_date!));
  // On-track: nearest deadline first
  onTrack.sort((a, b) => a.target_date!.localeCompare(b.target_date!));

  return { overdue, onTrack, noDates };
}

interface TickCluster {
  /** X position as fraction of bar width (0-1). */
  relX: number;
  count: number;
  overdue: boolean;
}

/** Cluster issue ticks that are within `threshold` fraction of each other. */
function clusterTicks(
  issues: ChartIssue[],
  parentStart: number,
  parentRange: number,
  threshold: number,
): TickCluster[] {
  const dated = issues.filter((i) => i.target_date);
  if (dated.length === 0) return [];

  // Sort by target_date
  const sorted = [...dated].sort((a, b) =>
    a.target_date!.localeCompare(b.target_date!),
  );

  const clusters: TickCluster[] = [];
  let curCluster: { relXs: number[]; overdueCount: number } = {
    relXs: [],
    overdueCount: 0,
  };

  for (const issue of sorted) {
    const relX = (parseDate(issue.target_date!) - parentStart) / parentRange;
    if (
      curCluster.relXs.length > 0 &&
      relX - curCluster.relXs[curCluster.relXs.length - 1] > threshold
    ) {
      // Flush current cluster
      const avg =
        curCluster.relXs.reduce((a, b) => a + b, 0) / curCluster.relXs.length;
      clusters.push({
        relX: avg,
        count: curCluster.relXs.length,
        overdue: curCluster.overdueCount > 0,
      });
      curCluster = { relXs: [], overdueCount: 0 };
    }
    curCluster.relXs.push(relX);
    // Mark overdue if tick is beyond parent bar end (relX > 1.0)
    if (relX > 1.0) curCluster.overdueCount++;
  }

  // Flush last cluster
  if (curCluster.relXs.length > 0) {
    const avg =
      curCluster.relXs.reduce((a, b) => a + b, 0) / curCluster.relXs.length;
    clusters.push({
      relX: avg,
      count: curCluster.relXs.length,
      overdue: curCluster.overdueCount > 0,
    });
  }

  return clusters;
}

/** Format an overdue duration as "+N days" or "+N wks". */
function formatOverdue(targetDate: string, nodeEnd: string): string {
  const target = parseDate(targetDate);
  const end = parseDate(nodeEnd);
  const diffMs = target - end;
  if (diffMs <= 0) return "";
  const diffDays = Math.ceil(diffMs / (24 * 3600 * 1000));
  if (diffDays < 14) return `+${diffDays} days`;
  const diffWeeks = Math.round(diffDays / 7);
  return `+${diffWeeks} wks`;
}

// ---------------------------------------------------------------------------
// Breadcrumb
// ---------------------------------------------------------------------------

function updateBreadcrumb(): void {
  const parts: { label: string; path: string }[] = [
    { label: data.org_name || "Root", path: "" },
  ];
  if (currentPath !== "") {
    const segments = currentPath.split("/");
    let accumulated = "";
    for (const seg of segments) {
      accumulated = accumulated ? `${accumulated}/${seg}` : seg;
      const node = findNode(data.nodes, accumulated);
      parts.push({ label: node?.name || seg, path: accumulated });
    }
  }

  breadcrumbEl.innerHTML = parts
    .map((p, i) => {
      if (i === parts.length - 1) {
        return `<span class="crumb-current">${p.label}</span>`;
      }
      return `<span class="crumb" onclick="window.__nav('${p.path}')">${p.label}</span>`;
    })
    .join('<span class="crumb-sep"> &rsaquo; </span>');
}

// ---------------------------------------------------------------------------
// Detail Panel
// ---------------------------------------------------------------------------

function escapeHtml(s: string): string {
  const div = document.createElement("div");
  div.textContent = s;
  return div.innerHTML;
}

function issueUrl(ref: string): string {
  // "owner/repo#123" → "https://github.com/owner/repo/issues/123"
  const match = ref.match(/^(.+?)\/(.+?)#(\d+)$/);
  if (!match) return "#";
  return `https://github.com/${match[1]}/${match[2]}/issues/${match[3]}`;
}

function showPanel(node: ChartNode): void {
  selectedNode = node;
  let html = "";

  // Name
  html += `<h2>${escapeHtml(node.name)}</h2>`;

  // Status badge
  html += `<span class="panel-status ${node.status}">${node.status}</span>`;

  // Description
  if (node.description) {
    html += `<div class="panel-section">`;
    html += `<h3>Description</h3>`;
    html += `<div class="panel-desc">${escapeHtml(node.description)}</div>`;
    html += `</div>`;
  }

  // Timeline
  html += `<div class="panel-section">`;
  html += `<h3>Timeline</h3>`;
  html += `<div class="panel-meta">`;
  if (node.has_timeline) {
    html += `<span class="label">Start:</span> ${node.start}<br/>`;
    html += `<span class="label">End:</span> ${node.end}`;
  } else if (node.eff_start) {
    html += `<span class="label">Derived:</span> ${node.eff_start} &rarr; ${node.eff_end}`;
  } else {
    html += `<span class="label">No timeline</span>`;
  }
  html += `</div></div>`;

  // Owners & Team
  if (node.owners.length > 0 || node.team) {
    html += `<div class="panel-section">`;
    html += `<h3>People</h3>`;
    html += `<div class="panel-meta">`;
    if (node.owners.length > 0) {
      html += `<span class="label">Owners:</span> ${node.owners.map(escapeHtml).join(", ")}<br/>`;
    }
    if (node.team) {
      html += `<span class="label">Team:</span> ${escapeHtml(node.team)}`;
    }
    html += `</div></div>`;
  }

  // Milestones (checkpoints)
  const checkpoints = allCheckpoints(node);
  if (checkpoints.length > 0) {
    html += `<div class="panel-section">`;
    html += `<h3>Milestones</h3>`;
    html += `<ul class="panel-milestones">`;
    for (const m of checkpoints) {
      html += `<li>&diams; ${escapeHtml(m.name)} <span class="ms-date">${m.date}</span>`;
      if (m.description) html += `<br/><span class="ms-date">${escapeHtml(m.description)}</span>`;
      html += `</li>`;
    }
    html += `</ul></div>`;
  }

  // Issues
  if (node.issues.length > 0) {
    html += `<div class="panel-section">`;
    html += `<h3>Issues (${node.issues.length})</h3>`;
    html += `<ul class="panel-issues">`;
    for (const issue of node.issues) {
      const url = issueUrl(issue.issue_ref);
      const label = issue.title
        ? `${escapeHtml(issue.title)} <span class="issue-ref">${escapeHtml(issue.issue_ref)}</span>`
        : escapeHtml(issue.issue_ref);
      html += `<li><a class="panel-issue-link" href="${url}" target="_blank" rel="noopener">${label}</a></li>`;
    }
    html += `</ul></div>`;
  }

  // Children
  if (node.children.length > 0) {
    html += `<div class="panel-section">`;
    html += `<h3>Children (${node.children.length})</h3>`;
    html += `<ul class="panel-children">`;
    for (const c of node.children) {
      const color = STATUS_COLORS[c.status] || STATUS_COLORS.active;
      const dates = c.has_timeline ? `${c.start} &rarr; ${c.end}` : c.eff_start ? `~${c.eff_start}` : "";
      html += `<li>`;
      html += `<span class="dot" style="background:${color}"></span>`;
      html += `<span class="child-name">${escapeHtml(c.name)}</span>`;
      if (dates) html += `<span class="child-dates">${dates}</span>`;
      html += `</li>`;
    }
    html += `</ul>`;
    html += `<button class="btn-drill" onclick="window.__nav('${node.path}')">Drill into ${escapeHtml(node.name)} &rsaquo;</button>`;
    html += `</div>`;
  }

  panelContentEl.innerHTML = html;
  panelEl.classList.add("open");
  chart.resize();
}

function closePanel(): void {
  selectedNode = null;
  panelEl.classList.remove("open");
  chart.resize();
}

// Expose to HTML onclick handlers
(window as any).__closePanel = closePanel;

// ---------------------------------------------------------------------------
// Chart rendering
// ---------------------------------------------------------------------------

function buildOption(): echarts.EChartsOption {
  const nodes = getVisibleNodes();
  visibleNodes = nodes;

  const [xMin, xMax] = computeTimeRange(nodes);

  const categories = nodes.map((n) => n.name).reverse();

  const seriesData = nodes.map((n, i) => ({
    value: [
      n.eff_start ? parseDate(n.eff_start) : xMin,
      n.eff_end ? parseDate(n.eff_end) : xMax,
      categories.length - 1 - i,
    ],
  }));

  // OKRs: org-wide full-height vertical lines (always visible)
  const okrs = collectOkrs(data.nodes);
  const okrLines = okrs.map((m) => ({
    xAxis: parseDate(m.date),
    name: m.name,
    _okr: m,
  }));

  // When drilled into a node, show that node's subtree checkpoints
  // as full-height lines (they're project-wide context at this level).
  const parentCheckpointLines: typeof okrLines = [];
  if (currentPath !== "") {
    const parentNode = findNode(data.nodes, currentPath);
    if (parentNode) {
      const seen = new Set(okrs.map((m) => `${m.name}|${m.date}`));
      for (const m of allCheckpoints(parentNode)) {
        const key = `${m.name}|${m.date}`;
        if (!seen.has(key)) {
          seen.add(key);
          parentCheckpointLines.push({
            xAxis: parseDate(m.date),
            name: m.name,
            _okr: m, // reuse same shape for tooltip
          });
        }
      }
    }
  }
  // "Today" marker
  const todayLine = {
    xAxis: new Date().setHours(0, 0, 0, 0),
    name: "Today",
    _okr: null as ChartMilestone | null,
  };

  const allVerticalLines = [todayLine, ...okrLines, ...parentCheckpointLines];

  return {
    tooltip: {
      trigger: "item",
      formatter: (params: any) => {
        const idx = params.dataIndex;
        const n = visibleNodes[idx];
        if (!n) return "";
        const dates = n.has_timeline
          ? `${n.start} &rarr; ${n.end}`
          : n.eff_start
            ? `~${n.eff_start} &rarr; ~${n.eff_end} (derived)`
            : "No fixed timeline";
        const parts = [`<b>${n.name}</b>`, dates, `Status: ${n.status}`];
        if (n.owners.length > 0) parts.push(`Owners: ${n.owners.join(", ")}`);
        if (n.team) parts.push(`Team: ${n.team}`);
        const ms = allCheckpoints(n);
        if (ms.length > 0) {
          parts.push("");
          parts.push(`<b>Milestones:</b>`);
          for (const m of ms) {
            parts.push(`&diams; ${m.name} (${m.date})`);
          }
        }
        parts.push("", "<i>Click for details</i>");
        return parts.join("<br/>");
      },
    },
    grid: {
      top: 40,
      bottom: 40,
      left: 20,
      right: 20,
      containLabel: true,
    },
    xAxis: {
      type: "time",
      min: xMin,
      max: xMax,
      axisLabel: { color: "#8b949e" },
      axisLine: { lineStyle: { color: "#30363d" } },
      splitLine: {
        show: true,
        lineStyle: { color: "#21262d", type: "dashed" },
      },
    },
    yAxis: {
      type: "category",
      data: categories,
      axisLabel: {
        color: "#e6edf3",
        fontWeight: "bold",
        fontSize: 13,
      },
      axisLine: { show: false },
      axisTick: { show: false },
    },
    series: [
      {
        type: "custom",
        renderItem: renderBar,
        encode: { x: [0, 1], y: 2 },
        data: seriesData,
      },
      // Invisible line series that carries markLine for vertical lines
      // (today, OKRs, checkpoints). markLine on custom series is unreliable.
      {
        type: "line",
        data: [],
        symbol: "none",
        silent: true,
        markLine: {
          silent: false,
          symbol: ["none", "none"],
          label: {
            show: true,
            position: "start",
            formatter: (p: any) => p.name,
            fontSize: 10,
            color: "#8b949e",
          },
          lineStyle: {
            type: "dashed",
            width: 1,
          },
          data: allVerticalLines.map((line) => {
            const isToday = line === todayLine;
            const isOkr = okrLines.includes(line);
            if (isToday) {
              return {
                ...line,
                lineStyle: {
                  color: "rgba(239, 68, 68, 0.7)",
                  type: "solid" as const,
                  width: 2,
                },
                label: {
                  color: "#ef4444",
                },
              };
            }
            return {
              ...line,
              lineStyle: {
                color: isOkr
                  ? "rgba(167, 139, 250, 0.5)"
                  : "rgba(245, 158, 11, 0.5)",
              },
              label: {
                color: isOkr ? "#a78bfa" : "#f59e0b",
              },
            };
          }),
          tooltip: {
            formatter: (p: any) => {
              if (p.name === "Today") return "Today";
              const m = p.data?._okr as ChartMilestone | undefined;
              if (!m) return p.name;
              return [
                `<b>${m.name}</b>`,
                m.date,
                m.description || "",
              ]
                .filter(Boolean)
                .join("<br/>");
            },
          },
        },
      },
    ],
    backgroundColor: "transparent",
  };
}

/** Custom renderItem: draws the outer bar + nested child bars. */
function renderBar(
  params: any,
  api: any,
): echarts.CustomSeriesRenderItemReturn {
  const node: ChartNode = visibleNodes[params.dataIndex];
  if (!node) return { type: "group", children: [] };

  const yIdx = api.value(2);
  const start = api.coord([api.value(0), yIdx]);
  const end = api.coord([api.value(1), yIdx]);
  const bandWidth = api.size([0, 1])[1];

  const x = start[0];
  const y = start[1] - bandWidth * 0.4;
  const width = end[0] - start[0];
  const height = bandWidth * 0.8;

  if (width <= 0) return { type: "group", children: [] };

  const children: any[] = [];

  // Highlight selected node
  const isSelected = selectedNode?.path === node.path;

  // Outer bar
  const statusColor = STATUS_COLORS[node.status] || STATUS_COLORS.active;
  const hasTimeline = node.has_timeline;
  children.push({
    type: "rect",
    shape: { x, y, width, height, r: 4 },
    style: {
      fill: hasTimeline ? `${statusColor}22` : NO_TIMELINE_COLOR,
      stroke: isSelected
        ? "#e6edf3"
        : hasTimeline
          ? `${statusColor}55`
          : NO_TIMELINE_BORDER,
      lineWidth: isSelected ? 2 : 1,
      lineDash: hasTimeline || isSelected ? null : [4, 3],
    },
  });

  // Heat bar fill for non-leaf nodes
  if (node.children.length > 0) {
    const childrenWithTimeline = node.children.filter(
      (c) => c.eff_start && c.eff_end,
    );

    if (childrenWithTimeline.length > 0) {
      const outerStart = api.value(0) as number;
      const outerEnd = api.value(1) as number;
      const outerRange = outerEnd - outerStart;

      const childStarts = childrenWithTimeline.map((c) =>
        parseDate(c.eff_start!),
      );
      const childEnds = childrenWithTimeline.map((c) =>
        parseDate(c.eff_end!),
      );
      const spanStart = Math.min(...childStarts);
      const spanEnd = Math.max(...childEnds);

      const relStart = Math.max(0, (spanStart - outerStart) / outerRange);
      const relEnd = Math.min(1, (spanEnd - outerStart) / outerRange);

      const fillX = x + relStart * width;
      const fillW = (relEnd - relStart) * width;

      // Gradient fill rectangle
      children.push({
        type: "rect",
        shape: { x: fillX, y: y + 1, width: fillW, height: height - 2, r: 4 },
        style: {
          fill: new (echarts.graphic as any).LinearGradient(0, 0, 1, 0, [
            { offset: 0, color: "rgba(88, 166, 255, 0.35)" },
            { offset: 1, color: "rgba(88, 166, 255, 0.15)" },
          ]),
        },
      });
    }

    // Count badge
    const badgeText = `×${node.children.length} children`;
    // Position badge to the left of the drillable arrow (arrow is at x+width-12)
    const badgeX = x + width - 20;
    const badgeY = y + height / 2;

    children.push({
      type: "text",
      style: {
        text: badgeText,
        x: badgeX,
        y: badgeY,
        fill: "#58a6ff",
        fontSize: 11,
        fontWeight: 600,
        textAlign: "right",
        textVerticalAlign: "middle",
        backgroundColor: "rgba(13, 17, 23, 0.7)",
        borderRadius: 3,
        padding: [2, 6],
      },
    });
  }

  // Issue tick marks for leaf nodes (no children, has issues)
  if (node.children.length === 0 && node.issues.length > 0) {
    const outerStart = api.value(0) as number;
    const outerEnd = api.value(1) as number;
    const outerRange = outerEnd - outerStart;

    const clusters = clusterTicks(node.issues, outerStart, outerRange, 0.02);

    for (const cluster of clusters) {
      const clampedX = Math.max(0, Math.min(1, cluster.relX));
      const tickX = x + clampedX * width;
      const tickW = cluster.count > 1 ? 6 : 3;
      const tickH = 14;
      const tickY = y + (height - tickH) / 2;
      const tickColor = cluster.overdue ? "#f85149" : "#58a6ff";
      const tickOpacity = cluster.overdue ? 0.9 : 0.7;

      children.push({
        type: "rect",
        shape: {
          x: tickX - tickW / 2,
          y: tickY,
          width: tickW,
          height: tickH,
          r: 1,
        },
        style: {
          fill: tickColor,
          opacity: tickOpacity,
        },
      });

      // Show count above clustered ticks
      if (cluster.count > 1) {
        children.push({
          type: "text",
          style: {
            text: `${cluster.count}`,
            x: tickX,
            y: tickY - 2,
            fill: tickColor,
            fontSize: 8,
            textAlign: "center",
            textVerticalAlign: "bottom",
            opacity: 0.8,
          },
        });
      }
    }

    // Summary badge
    const overdueCount = node.issues.filter(
      (i) => i.target_date && node.end && i.target_date > node.end,
    ).length;
    const badgeX = x + width - 8;
    const badgeY = y + height / 2;

    // Use a rich text label to color the overdue part differently
    if (overdueCount > 0) {
      children.push({
        type: "text",
        style: {
          text: `${node.issues.length} issues · {overdue|${overdueCount} overdue}`,
          x: badgeX,
          y: badgeY,
          fill: "#8b949e",
          fontSize: 11,
          fontWeight: 600,
          textAlign: "right",
          textVerticalAlign: "middle",
          backgroundColor: "rgba(13, 17, 23, 0.7)",
          borderRadius: 3,
          padding: [2, 6],
          rich: {
            overdue: {
              fill: "#f85149",
              fontSize: 11,
              fontWeight: 600,
            },
          },
        },
      });
    } else {
      children.push({
        type: "text",
        style: {
          text: `${node.issues.length} issues`,
          x: badgeX,
          y: badgeY,
          fill: "#8b949e",
          fontSize: 11,
          fontWeight: 600,
          textAlign: "right",
          textVerticalAlign: "middle",
          backgroundColor: "rgba(13, 17, 23, 0.7)",
          borderRadius: 3,
          padding: [2, 6],
        },
      });
    }
  }

  // Checkpoint markers (diamonds + dashed vertical line within this row)
  // Only show per-row markers at the root level. When drilled in,
  // checkpoints are already displayed as full-height project-wide lines.
  const nodeMilestones = currentPath === "" ? allCheckpoints(node) : [];
  if (nodeMilestones.length > 0) {
    for (const m of nodeMilestones) {
      const mDate = parseDate(m.date);
      // Position milestone on the time axis directly, not relative to the bar
      const mx = api.coord([mDate, api.value(2)])[0];
      const mColor = "#f59e0b";

      children.push({
        type: "line",
        shape: { x1: mx, y1: y, x2: mx, y2: y + height },
        style: {
          stroke: mColor,
          lineWidth: 1,
          lineDash: [3, 2],
          opacity: 0.7,
        },
      });

      const ds = 5;
      const dy = y + 2 + ds;
      children.push({
        type: "path",
        shape: {
          d: `M${mx},${dy - ds}L${mx + ds},${dy}L${mx},${dy + ds}L${mx - ds},${dy}Z`,
        },
        style: {
          fill: mColor,
          opacity: 0.9,
        },
      });

      children.push({
        type: "text",
        style: {
          text: m.name,
          x: mx,
          y: y - 2,
          fill: mColor,
          fontSize: 9,
          textAlign: "center",
          textVerticalAlign: "bottom",
          opacity: 0.8,
        },
      });
    }
  }

  // Drillable indicator (small arrow if has children)
  if (node.children.length > 0) {
    const arrowX = x + width - 12;
    const arrowY = y + height / 2;
    children.push({
      type: "path",
      shape: {
        d: `M${arrowX},${arrowY - 4}L${arrowX + 6},${arrowY}L${arrowX},${arrowY + 4}Z`,
      },
      style: {
        fill: hasTimeline ? statusColor : "#6b7280",
        opacity: 0.6,
      },
    });
  }

  return { type: "group", children };
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

function navigateTo(path: string): void {
  currentPath = path;
  selectedNode = null;
  closePanel();
  updateBreadcrumb();
  renderChart();
}

// Expose to inline onclick handlers
(window as any).__nav = navigateTo;

// Single click: select node and show panel
chart.on("click", (params: any) => {
  const node: ChartNode | undefined = visibleNodes[params.dataIndex];
  if (node) {
    showPanel(node);
    renderChart();
  }
});

// Double click: drill into node
chart.on("dblclick", (params: any) => {
  const node: ChartNode | undefined = visibleNodes[params.dataIndex];
  if (node && node.children.length > 0) {
    navigateTo(node.path);
  }
});

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

function renderChart(): void {
  chart.setOption(buildOption(), true);
}

// Toggle range button
toggleBtn.addEventListener("click", () => {
  useGlobalRange = !useGlobalRange;
  toggleBtn.textContent = useGlobalRange ? "Show Fitted Range" : "Show Global Range";
  renderChart();
});

// Responsive resize
window.addEventListener("resize", () => chart.resize());

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

updateBreadcrumb();
renderChart();
