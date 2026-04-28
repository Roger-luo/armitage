/**
 * Chart entry point: state management, rendering orchestration, event wiring.
 */

import type { ChartData, ChartNode, ChartMilestone, ChartIssue } from "./types";
import { getLayoutElements, getTimelineWidth, syncSvgHeight, getAxisHeight, getBarsTop, getRowHeight, type LayoutElements } from "./layout";
import { createScale, setupZoom, resetZoom, updateScaleRange, parseDate, type ScaleState } from "./scale";
import { renderAxis, renderGridLines, renderTodayLine, renderMilestoneLines } from "./render-axis";
import { renderNodeRow, sortIssues, formatOverdue, type RenderedRow } from "./render-nodes";
import { renderIssueRows, issueUrl } from "./render-issues";

declare const marked: { parse(src: string): string };

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

/**
 * Collect milestones relevant to the current view.
 *
 * Scoping rules:
 * - Top-level (currentPath = ""): all milestones from the entire tree.
 * - Drilled into node P: milestones from P's subtree (P itself + all descendants)
 *   PLUS milestones from every ancestor of P (a milestone on a non-leaf node
 *   propagates to all its children, so ancestors apply to the viewed children).
 *   Milestones from sibling branches are excluded.
 *
 * `typeFilter`: "okr" | "checkpoint" | "all"
 */
function collectMilestonesForView(typeFilter: "okr" | "checkpoint" | "all"): ChartMilestone[] {
  const seen = new Set<string>();
  const result: ChartMilestone[] = [];

  function add(m: ChartMilestone) {
    const isOkr = m.milestone_type === "okr";
    if (typeFilter === "all" || (typeFilter === "okr" && isOkr) || (typeFilter === "checkpoint" && !isOkr)) {
      const key = `${m.name}|${m.date}`;
      if (!seen.has(key)) { seen.add(key); result.push(m); }
    }
  }

  function walkSubtree(n: ChartNode) {
    n.milestones.forEach(add);
    n.children.forEach(walkSubtree);
  }

  if (currentPath === "") {
    data.nodes.forEach(walkSubtree);
  } else {
    // Subtree rooted at currentPath
    const node = findNode(data.nodes, currentPath);
    if (node) walkSubtree(node);

    // Ancestor milestones: a milestone on a non-leaf propagates to all descendants.
    // The ancestors of "a/b/c" are "a" and "a/b".
    const parts = currentPath.split("/");
    for (let i = 1; i < parts.length; i++) {
      const ancestorPath = parts.slice(0, i).join("/");
      const ancestor = findNode(data.nodes, ancestorPath);
      if (ancestor) ancestor.milestones.forEach(add);
    }
  }

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

/**
 * Render a markdown string to HTML using marked, falling back to escaped plain text.
 * When repo is provided (e.g. "QuEra-QCS/GeminiSequences"), relative URLs and
 * bare issue references (#123) are resolved against that GitHub repo.
 */
function renderMarkdown(s: string, repo?: string): string {
  try {
    let html = marked.parse(s);
    if (repo) {
      const base = `https://github.com/${repo}`;
      // Rewrite relative href/src that start with ./ or don't start with http/# / mailto
      html = html.replace(
        /((?:href|src)=["'])(?!https?:\/\/|mailto:|#)(\.\/)?(.*?)(["'])/g,
        (_, prefix, _dot, path, suffix) => `${prefix}${base}/blob/main/${path}${suffix}`,
      );
      // Link bare #123 issue references (not already inside an href)
      html = html.replace(
        /(?<!["\/\w])#(\d+)\b/g,
        `<a href="${base}/issues/$1" target="_blank" rel="noopener">#$1</a>`,
      );
    }
    return html;
  } catch {
    return `<p>${escapeHtml(s)}</p>`;
  }
}

/**
 * Post-process a panel description element: replace broken images with
 * clickable "View image on GitHub" links, since GitHub user-attachment
 * URLs require authentication and get blocked by ORB cross-origin.
 */
function fixBrokenImages(container: HTMLElement, issueUrl: string): void {
  const imgs = container.querySelectorAll("img");
  for (const img of imgs) {
    img.addEventListener("error", () => {
      const link = document.createElement("a");
      link.href = issueUrl;
      link.target = "_blank";
      link.rel = "noopener";
      link.className = "broken-img-link";
      link.textContent = `🖼 ${img.alt || "View image on GitHub"}`;
      img.replaceWith(link);
    });
  }
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
    html += `<div class="panel-section"><h3>Description</h3><div class="panel-desc">${renderMarkdown(node.description)}</div></div>`;
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
      const url = issueUrl(issue.issue_ref, issue.is_pr);
      const prBadge = issue.is_pr ? `<span class="panel-pr-badge">PR</span>` : "";
      const label = issue.title
        ? `${prBadge}${escapeHtml(issue.title)} <span class="issue-ref">${escapeHtml(issue.issue_ref)}</span>`
        : `${prBadge}${escapeHtml(issue.issue_ref)}`;
      html += `<li><a class="panel-issue-link" href="${url}" target="_blank" rel="noopener">${label}</a></li>`;
    }
    html += `</ul></div>`;
  }

  panelContentEl.innerHTML = html;
  panelEl.classList.add("open");
}

function showIssuePanel(issue: ChartIssue, parentNode: ChartNode): void {
  selectedNode = null;
  const url = issueUrl(issue.issue_ref, issue.is_pr);
  const isOverdue = issue.target_date && parentNode.end && issue.target_date > parentNode.end;

  let html = "";
  html += `<h2>${escapeHtml(issue.title || issue.issue_ref)}</h2>`;
  html += `<a class="panel-issue-link" href="${url}" target="_blank" rel="noopener">${escapeHtml(issue.issue_ref)} &rarr; Open on GitHub</a>`;
  html += `<span class="panel-status ${issue.state === "CLOSED" ? "completed" : "active"}">${(issue.state || "OPEN").toLowerCase()}</span>`;

  // Participants: author + assignees, deduplicated
  const participants = new Set<string>();
  if (issue.author) participants.add(issue.author);
  if (issue.assignees) {
    for (const a of issue.assignees) participants.add(a);
  }
  if (participants.size > 0) {
    html += `<div class="panel-section"><h3>Participants</h3>`;
    html += `<div class="panel-participants">`;
    for (const user of participants) {
      const isAuthor = user === issue.author;
      html += `<a class="panel-participant" href="https://github.com/${encodeURIComponent(user)}" target="_blank" rel="noopener">`;
      html += `@${escapeHtml(user)}`;
      if (isAuthor) html += ` <span class="participant-role">author</span>`;
      html += `</a>`;
    }
    html += `</div></div>`;
  }

  if (issue.labels && issue.labels.length > 0) {
    html += `<div class="panel-section"><h3>Labels</h3>`;
    html += `<div class="panel-labels">`;
    for (const label of issue.labels) {
      html += `<span class="panel-label">${escapeHtml(label)}</span>`;
    }
    html += `</div></div>`;
  }

  html += `<div class="panel-section"><h3>Timeline</h3><div class="panel-meta">`;
  if (issue.start_date) html += `<span class="label">Start:</span> ${issue.start_date}<br/>`;
  if (issue.target_date) html += `<span class="label">Target:</span> ${issue.target_date}`;
  if (isOverdue && parentNode.end) {
    html += `<br/><span class="issue-overflow">Overdue: ${formatOverdue(issue.target_date!, parentNode.end)} past ${escapeHtml(parentNode.name)} deadline</span>`;
  }
  html += `</div></div>`;

  html += `<div class="panel-section"><h3>Parent</h3>`;
  html += `<div class="panel-meta"><span class="crumb" onclick="window.__nav('${parentNode.path}')">${escapeHtml(parentNode.name)}</span></div></div>`;

  if (issue.description) {
    const repoMatch = issue.issue_ref.match(/^(.+?\/.+?)#/);
    const repo = repoMatch ? repoMatch[1] : undefined;
    html += `<div class="panel-section"><h3>Description</h3>`;
    html += `<div class="panel-desc">${renderMarkdown(issue.description, repo)}</div>`;
    html += `</div>`;
  }

  panelContentEl.innerHTML = html;
  panelEl.classList.add("open");
  fixBrokenImages(panelContentEl, url);
}

function closePanel(): void {
  selectedNode = null;
  panelEl.classList.remove("open");
}

function showMilestonePanel(m: ChartMilestone): void {
  const typeLabel = m.milestone_type === "okr" ? "OKR" : "Checkpoint";
  const color = m.milestone_type === "okr" ? "#a78bfa" : "#f59e0b";
  let html = `<h2 style="color:${color}">${escapeHtml(m.name)}</h2>`;
  html += `<span class="panel-status active" style="background:none;color:${color}">${typeLabel}</span>`;
  html += `<div class="panel-section"><h3>Date</h3><div class="panel-meta">${escapeHtml(m.date)}</div></div>`;
  if (m.description) {
    html += `<div class="panel-section"><h3>Description</h3><div class="panel-desc">${renderMarkdown(m.description)}</div></div>`;
  }
  panelContentEl.innerHTML = html;
  panelEl.classList.add("open");
}

(window as any).__openMilestonePanel = showMilestonePanel;
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

  // Build ancestor chain for timeline inheritance (walk up from current path)
  const ancestors: ChartNode[] = [];
  if (currentPath) {
    const segments = currentPath.split("/");
    let accumulated = "";
    for (const seg of segments) {
      accumulated = accumulated ? `${accumulated}/${seg}` : seg;
      const ancestor = findNode(data.nodes, accumulated);
      if (ancestor) ancestors.push(ancestor);
    }
    ancestors.reverse(); // immediate parent first
  }

  // Render rows
  let yOffset = 0;
  for (const node of nodes) {
    const isDimmed = expandedNode !== null && expandedNode !== node.path;
    const isExpanded = expandedNode === node.path;

    const row = renderNodeRow(node, scaleState, layout, yOffset, { isDimmed, isExpanded, parentNode: ancestors[0] || null });
    renderedRows.push(row);
    yOffset += row.height;

    // Expanded issue rows — pass the node itself + ancestors for inheritance
    if (isExpanded && node.issues.length > 0) {
      const issueRows = renderIssueRows(node, scaleState, layout, yOffset, expandedShowAll, ancestors);
      renderedRows.push(...issueRows);
      yOffset += issueRows.reduce((sum, r) => sum + r.height, 0);
    }
  }

  // Milestone lines — scoped to the current view (see collectMilestonesForView).
  const milestones = collectMilestonesForView("all");
  const barsTop = getBarsTop(milestones.length > 0);

  // Sync SVG height and bar position (barsTop accounts for the milestone label zone).
  syncSvgHeight(layout, yOffset, barsTop);
  // Keep the label column aligned with the bar rows.
  layout.labelsEl.style.paddingTop = `${barsTop}px`;

  const totalHeight = yOffset + barsTop;
  renderAxis(scaleState, layout, totalHeight, barsTop);
  renderGridLines(scaleState, layout, totalHeight, barsTop);
  renderTodayLine(scaleState, layout, totalHeight, barsTop);
  renderMilestoneLines(scaleState, layout, totalHeight, milestones, barsTop);
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

// Also handle clicks anywhere in the SVG timeline row region
function findRowFromSvgY(e: MouseEvent): RenderedRow | undefined {
  const svg = layout.timelineSvg;
  const pt = svg.createSVGPoint();
  pt.x = e.clientX;
  pt.y = e.clientY;
  const svgY = pt.matrixTransform(svg.getScreenCTM()!.inverse()).y;
  const barsTop = getBarsTop(collectMilestonesForView("all").length > 0);
  // Ignore clicks in the axis / milestone-label zone above the bars
  if (svgY < barsTop) return undefined;
  const barsRelY = svgY - barsTop;
  for (const row of renderedRows) {
    if (barsRelY >= row.y && barsRelY < row.y + row.height) return row;
  }
  return undefined;
}

layout.timelineSvg.addEventListener("click", (e) => {
  const row = findRowFromSvgY(e as MouseEvent);
  if (row) handleRowClick(row);
});

layout.timelineSvg.addEventListener("dblclick", (e) => {
  const row = findRowFromSvgY(e as MouseEvent);
  if (row) handleRowDblClick(row);
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

// Wire hover highlight on SVG timeline → highlight corresponding label row + bar
let hoveredSvgRow: RenderedRow | null = null;

function highlightRow(row: RenderedRow): void {
  // Highlight label row
  const idx = renderedRows.indexOf(row);
  if (idx >= 0) {
    const labelRow = layout.labelsEl.children[idx] as HTMLElement | undefined;
    if (labelRow) labelRow.classList.add("highlighted");
  }
  // Highlight SVG bars
  if (row.type === "issue" && row.issue) {
    layout.barsGroup.querySelectorAll(`.issue-bar[data-issue-ref="${CSS.escape(row.issue.issue_ref)}"]`)
      .forEach((el) => el.classList.add("highlighted"));
  } else if (row.type === "node" && row.node) {
    layout.barsGroup.querySelectorAll(`.node-bar[data-path="${CSS.escape(row.node.path)}"]`)
      .forEach((el) => el.classList.add("highlighted"));
  }
}

function unhighlightRow(row: RenderedRow): void {
  const idx = renderedRows.indexOf(row);
  if (idx >= 0) {
    const labelRow = layout.labelsEl.children[idx] as HTMLElement | undefined;
    if (labelRow) labelRow.classList.remove("highlighted");
  }
  if (row.type === "issue" && row.issue) {
    layout.barsGroup.querySelectorAll(`.issue-bar[data-issue-ref="${CSS.escape(row.issue.issue_ref)}"]`)
      .forEach((el) => el.classList.remove("highlighted"));
  } else if (row.type === "node" && row.node) {
    layout.barsGroup.querySelectorAll(`.node-bar[data-path="${CSS.escape(row.node.path)}"]`)
      .forEach((el) => el.classList.remove("highlighted"));
  }
}

layout.timelineSvg.addEventListener("mousemove", (e) => {
  const row = findRowFromSvgY(e as MouseEvent);
  if (row === hoveredSvgRow) return;
  if (hoveredSvgRow) { unhighlightRow(hoveredSvgRow); hideTooltip(); }
  hoveredSvgRow = row || null;
  if (!row) return;
  highlightRow(row);

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
});

layout.timelineSvg.addEventListener("mouseleave", () => {
  if (hoveredSvgRow) { unhighlightRow(hoveredSvgRow); hoveredSvgRow = null; }
  hideTooltip();
});

// Resize handler
window.addEventListener("resize", () => {
  updateScaleRange(scaleState, getTimelineWidth());
  renderChart();
});

// Initial render
updateBreadcrumb();
renderChart();
