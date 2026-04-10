/**
 * Renders node rows: HTML labels in the label column + SVG bars in the timeline.
 */

import type { ChartNode, ChartIssue } from "./types";
import type { ScaleState } from "./scale";
import type { LayoutElements } from "./layout";
import { getRowHeight, getAxisHeight } from "./layout";
import { parseDate, dateToX } from "./scale";

const STATUS_COLORS: Record<string, string> = {
  active: "#3b82f6",
  completed: "#6b7280",
  paused: "#f59e0b",
  cancelled: "#ef4444",
};

/** Sort issues into overdue / on-track / no-dates buckets. */
export interface SortedIssues {
  overdue: ChartIssue[];
  onTrack: ChartIssue[];
  noDates: ChartIssue[];
}

export function sortIssues(issues: ChartIssue[], nodeEnd: string | null): { overdue: ChartIssue[]; onTrack: ChartIssue[]; noDates: ChartIssue[] } {
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

  overdue.sort((a, b) => b.target_date!.localeCompare(a.target_date!));
  onTrack.sort((a, b) => a.target_date!.localeCompare(b.target_date!));

  return { overdue, onTrack, noDates };
}

export interface RenderedRow {
  type: "node" | "issue" | "separator" | "show-more";
  node?: ChartNode;
  issue?: ChartIssue;
  parentNode?: ChartNode;
  labelEl: HTMLDivElement;
  y: number;
  height: number;
}

/**
 * Render a single node row: HTML label + SVG bar.
 * Returns the rendered row info for click handling.
 */
export function renderNodeRow(
  node: ChartNode,
  state: ScaleState,
  layout: LayoutElements,
  yOffset: number,
  options: { isDimmed: boolean; isExpanded: boolean },
): RenderedRow {
  const height = getRowHeight("node");

  // --- HTML label row ---
  const row = document.createElement("div");
  row.className = `chart-row node${options.isDimmed ? " dimmed" : ""}${options.isExpanded ? " expanded" : ""}`;
  row.style.height = `${height}px`;
  row.dataset.path = node.path;

  const label = document.createElement("span");
  label.className = "chart-label node-name";
  label.textContent = node.name;
  label.title = node.description || node.name;
  row.appendChild(label);

  // Badge
  if (node.children.length > 0) {
    const badge = document.createElement("span");
    badge.className = "chart-badge children";
    badge.textContent = `×${node.children.length}`;
    row.appendChild(badge);

    const arrow = document.createElement("span");
    arrow.className = "chart-drill";
    arrow.textContent = "▸";
    row.appendChild(arrow);
  } else if (node.issues.length > 0) {
    const badge = document.createElement("span");
    badge.className = "chart-badge issues";
    const overdueCount = node.issues.filter(
      (i) => i.target_date && node.end && i.target_date > node.end,
    ).length;
    if (overdueCount > 0) {
      badge.innerHTML = `${node.issues.length} issues · <span class="overdue-count">${overdueCount} overdue</span>`;
    } else {
      badge.textContent = `${node.issues.length} issues`;
    }
    row.appendChild(badge);
  }

  layout.labelsEl.appendChild(row);

  // --- SVG bar ---
  const statusColor = STATUS_COLORS[node.status] || STATUS_COLORS.active;
  const barY = yOffset;

  if (node.eff_start && node.eff_end) {
    const x1 = dateToX(state, node.eff_start);
    const x2 = dateToX(state, node.eff_end);
    const barW = Math.max(x2 - x1, 2);
    const barH = height - 8;
    const barTop = barY + 4;

    // Outer bar
    const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    rect.setAttribute("x", `${x1}`);
    rect.setAttribute("y", `${barTop}`);
    rect.setAttribute("width", `${barW}`);
    rect.setAttribute("height", `${barH}`);
    rect.setAttribute("rx", "4");
    rect.setAttribute("fill", node.has_timeline ? `${statusColor}22` : "rgba(107,114,128,0.15)");
    rect.setAttribute("stroke", options.isExpanded ? "rgba(88,166,255,0.6)" : (node.has_timeline ? `${statusColor}55` : "rgba(107,114,128,0.4)"));
    rect.setAttribute("stroke-width", options.isExpanded ? "2" : "1");
    if (!node.has_timeline && !options.isExpanded) {
      rect.setAttribute("stroke-dasharray", "4,3");
    }
    if (options.isDimmed) rect.setAttribute("opacity", "0.4");
    rect.dataset.path = node.path;
    layout.barsGroup.appendChild(rect);

    // Heat fill for non-leaf nodes
    if (node.children.length > 0) {
      const childrenWithTimeline = node.children.filter((c) => c.eff_start && c.eff_end);
      if (childrenWithTimeline.length > 0) {
        const spanStart = childrenWithTimeline.reduce((min, c) => c.eff_start! < min ? c.eff_start! : min, childrenWithTimeline[0].eff_start!);
        const spanEnd = childrenWithTimeline.reduce((max, c) => c.eff_end! > max ? c.eff_end! : max, childrenWithTimeline[0].eff_end!);
        const fillX1 = dateToX(state, spanStart);
        const fillX2 = dateToX(state, spanEnd);
        const fillW = Math.max(fillX2 - fillX1, 2);

        const fillRect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
        fillRect.setAttribute("x", `${fillX1}`);
        fillRect.setAttribute("y", `${barTop + 1}`);
        fillRect.setAttribute("width", `${fillW}`);
        fillRect.setAttribute("height", `${barH - 2}`);
        fillRect.setAttribute("rx", "3");
        fillRect.setAttribute("fill", "url(#heat-gradient)");
        if (options.isDimmed) fillRect.setAttribute("opacity", "0.4");
        layout.barsGroup.appendChild(fillRect);
      }
    }

    // Tick marks for leaf nodes with issues
    if (node.children.length === 0 && node.issues.length > 0) {
      const outerStart = parseDate(node.eff_start).getTime();
      const outerRange = parseDate(node.eff_end).getTime() - outerStart;

      for (const issue of node.issues) {
        if (!issue.target_date) continue;
        const tickX = dateToX(state, issue.target_date);
        const isOverdue = node.end && issue.target_date > node.end;
        const tickColor = isOverdue ? "#f85149" : "#58a6ff";

        const tick = document.createElementNS("http://www.w3.org/2000/svg", "rect");
        tick.setAttribute("x", `${tickX - 1.5}`);
        tick.setAttribute("y", `${barTop + (barH - 14) / 2}`);
        tick.setAttribute("width", "3");
        tick.setAttribute("height", "14");
        tick.setAttribute("rx", "1");
        tick.setAttribute("fill", tickColor);
        tick.setAttribute("opacity", isOverdue ? "0.9" : "0.7");
        if (options.isDimmed) tick.setAttribute("opacity", "0.3");
        layout.barsGroup.appendChild(tick);
      }
    }
  }

  return { type: "node", node, labelEl: row, y: yOffset, height };
}

/** Format an overdue duration as "+N days" or "+N wks". */
export function formatOverdue(targetDate: string, nodeEnd: string): string {
  const target = parseDate(targetDate).getTime();
  const end = parseDate(nodeEnd).getTime();
  const diffMs = target - end;
  if (diffMs <= 0) return "";
  const diffDays = Math.ceil(diffMs / (24 * 3600 * 1000));
  if (diffDays < 14) return `+${diffDays} days`;
  const diffWeeks = Math.round(diffDays / 7);
  return `+${diffWeeks} wks`;
}
