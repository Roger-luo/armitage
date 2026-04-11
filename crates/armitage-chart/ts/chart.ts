/**
 * Chart entry point: state management, rendering orchestration, event wiring.
 */

import type { ChartData, ChartNode, ChartMilestone, ChartIssue } from "./types";
import { getLayoutElements, getTimelineWidth, syncSvgHeight, getAxisHeight, getRowHeight, type LayoutElements } from "./layout";
import { createScale, setupZoom, resetZoom, updateScaleRange, parseDate, type ScaleState } from "./scale";
import { renderAxis, renderGridLines, renderTodayLine, renderMilestoneLines } from "./render-axis";
import { renderNodeRow, sortIssues, formatOverdue, type RenderedRow } from "./render-nodes";
import { renderIssueRows, issueUrl } from "./render-issues";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const data: ChartData = (window as any).__CHART_DATA__;
let currentPath = "";
let useGlobalRange = false;
let selectedNode: ChartNode | null = null;
let expandedNode: string | null = null;
let expandedShowAll = false;

let layout: LayoutElements;
let scaleState: ScaleState;
let renderedRows: RenderedRow[] = [];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function getVisibleNodes(): ChartNode[] {
  if (currentPath === "") return data.nodes;
  const node = findNode(data.nodes, currentPath);
  return node ? node.children : [];
}

function findNode(nodes: ChartNode[], path: string): ChartNode | null {
  for (const n of nodes) {
    if (n.path === path) return n;
    const found = findNode(n.children, path);
    if (found) return found;
  }
  return null;
}

function collectOkrs(nodes: ChartNode[]): ChartMilestone[] {
  const seen = new Set<string>();
  const result: ChartMilestone[] = [];
  function walk(ns: ChartNode[]) {
    for (const n of ns) {
      for (const m of n.milestones) {
        if (m.milestone_type === "okr") {
          const key = `${m.name}|${m.date}`;
          if (!seen.has(key)) { seen.add(key); result.push(m); }
        }
      }
      walk(n.children);
    }
  }
  walk(nodes);
  return result;
}

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

function computeTimeRange(nodes: ChartNode[]): [Date, Date] {
  if (useGlobalRange && data.global_start && data.global_end) {
    return [parseDate(data.global_start), parseDate(data.global_end)];
  }

  let min = Infinity;
  let max = -Infinity;
  for (const n of nodes) {
    if (n.eff_start) min = Math.min(min, parseDate(n.eff_start).getTime());
    if (n.eff_end) max = Math.max(max, parseDate(n.eff_end).getTime());
  }

  // Extend for expanded issue dates
  if (expandedNode) {
    const expNode = nodes.find((n) => n.path === expandedNode);
    if (expNode) {
      for (const issue of expNode.issues) {
        if (issue.start_date) min = Math.min(min, parseDate(issue.start_date).getTime());
        if (issue.target_date) max = Math.max(max, parseDate(issue.target_date).getTime());
      }
      if (expNode.overflow_end) {
        max = Math.max(max, new Date().setHours(0, 0, 0, 0));
      }
    }
  }

  if (min === Infinity || max === -Infinity) {
    const now = new Date();
    min = new Date(now.getFullYear(), 0, 1).getTime();
    max = new Date(now.getFullYear(), 11, 31).getTime();
  }

  const pad = 30 * 24 * 3600 * 1000;
  return [new Date(min - pad), new Date(max + pad)];
}

function escapeHtml(s: string): string {
  const div = document.createElement("div");
  div.textContent = s;
  return div.innerHTML;
}

// ---------------------------------------------------------------------------
// Side Panel
// ---------------------------------------------------------------------------

const panelEl = document.getElementById("panel")!;
const panelContentEl = document.getElementById("panel-content")!;

function showNodePanel(node: ChartNode): void {
  selectedNode = node;
  let html = "";
  html += `<h2>${escapeHtml(node.name)}</h2>`;
  html += `<span class="panel-status ${node.status}">${node.status}</span>`;

  if (node.description) {
    html += `<div class="panel-section"><h3>Description</h3><div class="panel-desc">${escapeHtml(node.description)}</div></div>`;
  }

  html += `<div class="panel-section"><h3>Timeline</h3><div class="panel-meta">`;
  if (node.has_timeline) {
    html += `<span class="label">Start:</span> ${node.start}<br/><span class="label">End:</span> ${node.end}`;
  } else if (node.eff_start) {
    html += `<span class="label">Derived:</span> ${node.eff_start} &rarr; ${node.eff_end}`;
  } else {
    html += `<span class="label">No timeline</span>`;
  }
  html += `</div></div>`;

  if (node.owners.length > 0 || node.team) {
    html += `<div class="panel-section"><h3>People</h3><div class="panel-meta">`;
    if (node.owners.length > 0) html += `<span class="label">Owners:</span> ${node.owners.map(escapeHtml).join(", ")}<br/>`;
    if (node.team) html += `<span class="label">Team:</span> ${escapeHtml(node.team)}`;
    html += `</div></div>`;
  }

  if (node.children.length > 0) {
    html += `<div class="panel-section"><h3>Children (${node.children.length})</h3>`;
    html += `<ul class="panel-children">`;
    for (const c of node.children) {
      html += `<li><span class="child-name">${escapeHtml(c.name)}</span></li>`;
    }
    html += `</ul>`;
    html += `<button class="btn-drill" onclick="window.__nav('${node.path}')">Drill into ${escapeHtml(node.name)} &rsaquo;</button>`;
    html += `</div>`;
  }

  if (node.issues.length > 0) {
    html += `<div class="panel-section"><h3>Issues (${node.issues.length})</h3>`;
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

  panelContentEl.innerHTML = html;
  panelEl.classList.add("open");
}

function showIssuePanel(issue: ChartIssue, parentNode: ChartNode): void {
  selectedNode = null;
  const url = issueUrl(issue.issue_ref);
  const isOverdue = issue.target_date && parentNode.end && issue.target_date > parentNode.end;

  let html = "";
  html += `<h2>${escapeHtml(issue.title || issue.issue_ref)}</h2>`;
  html += `<a class="panel-issue-link" href="${url}" target="_blank" rel="noopener">${escapeHtml(issue.issue_ref)} &rarr; Open on GitHub</a>`;
  html += `<span class="panel-status ${issue.state === "CLOSED" ? "completed" : "active"}">${(issue.state || "OPEN").toLowerCase()}</span>`;

  html += `<div class="panel-section"><h3>Timeline</h3><div class="panel-meta">`;
  if (issue.start_date) html += `<span class="label">Start:</span> ${issue.start_date}<br/>`;
  if (issue.target_date) html += `<span class="label">Target:</span> ${issue.target_date}`;
  if (isOverdue && parentNode.end) {
    html += `<br/><span class="issue-overflow">Overdue: ${formatOverdue(issue.target_date!, parentNode.end)} past ${escapeHtml(parentNode.name)} deadline</span>`;
  }
  html += `</div></div>`;

  html += `<div class="panel-section"><h3>Parent</h3>`;
  html += `<div class="panel-meta"><span class="crumb" onclick="window.__nav('${parentNode.path}')">${escapeHtml(parentNode.name)}</span></div></div>`;

  panelContentEl.innerHTML = html;
  panelEl.classList.add("open");
}

function closePanel(): void {
  selectedNode = null;
  panelEl.classList.remove("open");
}

(window as any).__closePanel = closePanel;

// ---------------------------------------------------------------------------
// Breadcrumb
// ---------------------------------------------------------------------------

const breadcrumbEl = document.getElementById("breadcrumb")!;

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
// Rendering
// ---------------------------------------------------------------------------

function renderChart(): void {
  const nodes = getVisibleNodes();
  const timeRange = computeTimeRange(nodes);
  const timelineWidth = getTimelineWidth();

  // Update scale
  scaleState.baseScale.domain(timeRange).range([0, timelineWidth]);
  scaleState.currentScale = scaleState.transform.rescaleX(scaleState.baseScale);

  // Clear previous content
  layout.labelsEl.innerHTML = "";
  layout.barsGroup.innerHTML = "";
  renderedRows = [];

  // Find parent node for timeline inheritance
  const parentNode = currentPath ? findNode(data.nodes, currentPath) : null;

  // Render rows
  let yOffset = 0;
  for (const node of nodes) {
    const isDimmed = expandedNode !== null && expandedNode !== node.path;
    const isExpanded = expandedNode === node.path;

    const row = renderNodeRow(node, scaleState, layout, yOffset, { isDimmed, isExpanded, parentNode });
    renderedRows.push(row);
    yOffset += row.height;

    // Expanded issue rows
    if (isExpanded && node.issues.length > 0) {
      const issueRows = renderIssueRows(node, scaleState, layout, yOffset, expandedShowAll);
      renderedRows.push(...issueRows);
      yOffset += issueRows.reduce((sum, r) => sum + r.height, 0);
    }
  }

  // Sync SVG height
  syncSvgHeight(layout, yOffset);

  // Render axis, grid, markers
  const totalHeight = yOffset + getAxisHeight();
  renderAxis(scaleState, layout, totalHeight);
  renderGridLines(scaleState, layout, totalHeight);
  renderTodayLine(scaleState, layout, totalHeight);

  // Milestone lines
  const okrs = collectOkrs(data.nodes);
  renderMilestoneLines(scaleState, layout, totalHeight, okrs);

  if (currentPath !== "") {
    const parentNode = findNode(data.nodes, currentPath);
    if (parentNode) {
      const checkpoints = allCheckpoints(parentNode);
      const filtered = checkpoints.filter((m) => !okrs.some((o) => o.name === m.name && o.date === m.date));
      renderMilestoneLines(scaleState, layout, totalHeight, filtered);
    }
  }
}

function onZoom(): void {
  // Full re-render with the updated scale (renderChart clears both columns)
  renderChart();
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

function navigateTo(path: string): void {
  currentPath = path;
  selectedNode = null;
  expandedNode = null;
  expandedShowAll = false;
  closePanel();
  updateBreadcrumb();
  resetZoom(scaleState, layout);
  renderChart();
}

(window as any).__nav = navigateTo;

// ---------------------------------------------------------------------------
// Click Handling
// ---------------------------------------------------------------------------

function handleRowClick(row: RenderedRow): void {
  if (row.type === "node" && row.node) {
    const node = row.node;
    if (node.children.length === 0 && node.issues.length > 0) {
      // Leaf node: toggle expand + show node panel
      if (expandedNode === node.path) {
        expandedNode = null;
        expandedShowAll = false;
        closePanel();
      } else {
        expandedNode = node.path;
        expandedShowAll = false;
        showNodePanel(node);
      }
      renderChart();
    } else {
      // Non-leaf: show panel
      showNodePanel(node);
    }
  } else if (row.type === "issue" && row.issue && row.parentNode) {
    showIssuePanel(row.issue, row.parentNode);
  } else if (row.type === "show-more" && row.parentNode) {
    if (expandedNode === row.parentNode.path) {
      expandedShowAll = true;
      renderChart();
    }
  }
}

function handleRowDblClick(row: RenderedRow): void {
  if (row.type === "node" && row.node && row.node.children.length > 0) {
    navigateTo(row.node.path);
  }
}

// ---------------------------------------------------------------------------
// Range toggle
// ---------------------------------------------------------------------------

function setRange(global: boolean): void {
  useGlobalRange = global;
  document.getElementById("btn-fitted")?.classList.toggle("active", !global);
  document.getElementById("btn-global")?.classList.toggle("active", global);
  resetZoom(scaleState, layout);
  renderChart();
}

(window as any).__setRange = setRange;

// ---------------------------------------------------------------------------
// Tooltip
// ---------------------------------------------------------------------------

const tooltipEl = document.getElementById("tooltip")!;

function showTooltip(e: MouseEvent, html: string): void {
  tooltipEl.innerHTML = html;
  tooltipEl.style.display = "block";
  tooltipEl.style.left = `${e.clientX + 12}px`;
  tooltipEl.style.top = `${e.clientY + 12}px`;
}

function hideTooltip(): void {
  tooltipEl.style.display = "none";
}

// ---------------------------------------------------------------------------
// Expose state for testing
// ---------------------------------------------------------------------------

(window as any).__chartState = {
  get currentPath() { return currentPath; },
  get expandedNode() { return expandedNode; },
  get visibleNodes() { return getVisibleNodes(); },
  get renderedRows() { return renderedRows; },
};

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

layout = getLayoutElements();
const initialRange = computeTimeRange(getVisibleNodes());
const initialWidth = getTimelineWidth();
scaleState = createScale(initialRange, initialWidth);

// Wire up zoom
setupZoom(scaleState, layout, onZoom);

// Wire up label click events via event delegation
layout.labelsEl.addEventListener("click", (e) => {
  const target = (e.target as HTMLElement).closest(".chart-row") as HTMLElement | null;
  if (!target) return;
  const idx = Array.from(layout.labelsEl.children).indexOf(target);
  if (idx >= 0 && renderedRows[idx]) {
    handleRowClick(renderedRows[idx]);
  }
});

layout.labelsEl.addEventListener("dblclick", (e) => {
  const target = (e.target as HTMLElement).closest(".chart-row") as HTMLElement | null;
  if (!target) return;
  const idx = Array.from(layout.labelsEl.children).indexOf(target);
  if (idx >= 0 && renderedRows[idx]) {
    handleRowDblClick(renderedRows[idx]);
  }
});

// Also handle clicks on SVG bars
layout.timelineSvg.addEventListener("click", (e) => {
  const target = e.target as SVGElement;
  const path = target.dataset?.path;
  if (path) {
    const row = renderedRows.find((r) => r.type === "node" && r.node?.path === path);
    if (row) handleRowClick(row);
  }
});

layout.timelineSvg.addEventListener("dblclick", (e) => {
  const target = e.target as SVGElement;
  const path = target.dataset?.path;
  if (path) {
    const row = renderedRows.find((r) => r.type === "node" && r.node?.path === path);
    if (row) handleRowDblClick(row);
  }
});

// Wire tooltip + hover highlight on label rows via event delegation
layout.labelsEl.addEventListener("mouseover", (e) => {
  const target = (e.target as HTMLElement).closest(".chart-row") as HTMLElement | null;
  if (!target) return;
  const idx = Array.from(layout.labelsEl.children).indexOf(target);
  const row = renderedRows[idx];
  if (!row) return;

  // Tooltip
  if (row.type === "issue" && row.issue) {
    const parts = [`<b>${escapeHtml(row.issue.title || row.issue.issue_ref)}</b>`, row.issue.issue_ref];
    if (row.issue.start_date) parts.push(`Start: ${row.issue.start_date}`);
    if (row.issue.target_date) parts.push(`Target: ${row.issue.target_date}`);
    if (row.issue.target_date && row.parentNode?.end && row.issue.target_date > row.parentNode.end) {
      parts.push(`<span style="color:#f85149">Overdue: ${formatOverdue(row.issue.target_date, row.parentNode.end)}</span>`);
    }
    showTooltip(e, parts.join("<br/>"));
  } else if (row.type === "node" && row.node) {
    const n = row.node;
    const dates = n.has_timeline ? `${n.start} → ${n.end}` : n.eff_start ? `~${n.eff_start} → ~${n.eff_end}` : "No timeline";
    showTooltip(e, `<b>${escapeHtml(n.name)}</b><br/>${dates}<br/>Status: ${n.status}`);
  }

  // Highlight corresponding SVG bar
  target.classList.add("highlighted");
  const issueRef = target.dataset.issueRef;
  const nodePath = target.dataset.path;
  if (issueRef) {
    layout.barsGroup.querySelectorAll(`.issue-bar[data-issue-ref="${CSS.escape(issueRef)}"]`)
      .forEach((el) => el.classList.add("highlighted"));
  } else if (nodePath) {
    layout.barsGroup.querySelectorAll(`.node-bar[data-path="${CSS.escape(nodePath)}"]`)
      .forEach((el) => el.classList.add("highlighted"));
  }
});

layout.labelsEl.addEventListener("mouseout", (e) => {
  const target = (e.target as HTMLElement).closest(".chart-row") as HTMLElement | null;
  if (!target) return;
  hideTooltip();

  // Remove all highlights
  target.classList.remove("highlighted");
  const issueRef = target.dataset.issueRef;
  const nodePath = target.dataset.path;
  if (issueRef) {
    layout.barsGroup.querySelectorAll(`.issue-bar[data-issue-ref="${CSS.escape(issueRef)}"]`)
      .forEach((el) => el.classList.remove("highlighted"));
  } else if (nodePath) {
    layout.barsGroup.querySelectorAll(`.node-bar[data-path="${CSS.escape(nodePath)}"]`)
      .forEach((el) => el.classList.remove("highlighted"));
  }
});

// Resize handler
window.addEventListener("resize", () => {
  updateScaleRange(scaleState, getTimelineWidth());
  renderChart();
});

// Initial render
updateBreadcrumb();
renderChart();
