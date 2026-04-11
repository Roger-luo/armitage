/**
 * Renders expanded issue rows below a leaf node.
 */

import type { ChartNode, ChartIssue } from "./types";
import type { ScaleState } from "./scale";
import type { LayoutElements } from "./layout";
import { getRowHeight, getAxisHeight } from "./layout";
import { dateToX, parseDate } from "./scale";
import { sortIssues, formatOverdue, type RenderedRow } from "./render-nodes";

const INITIAL_ISSUE_LIMIT = 7;

function issueUrl(ref: string): string {
  const match = ref.match(/^(.+?)\/(.+?)#(\d+)$/);
  if (!match) return "#";
  return `https://github.com/${match[1]}/${match[2]}/issues/${match[3]}`;
}

/**
 * Render expanded issue rows for a node.
 * Returns an array of RenderedRow for click handling.
 */
export function renderIssueRows(
  node: ChartNode,
  state: ScaleState,
  layout: LayoutElements,
  yOffset: number,
  showAll: boolean,
): RenderedRow[] {
  const rows: RenderedRow[] = [];
  const sorted = sortIssues(node.issues, node.end);
  const allSorted = [...sorted.overdue, ...sorted.onTrack, ...sorted.noDates];
  const limit = showAll ? allSorted.length : INITIAL_ISSUE_LIMIT;
  const visible = allSorted.slice(0, limit);

  let y = yOffset;
  let insertedOverdue = false;
  let insertedSeparator = false;

  for (const issue of visible) {
    const isOverdue = sorted.overdue.includes(issue);
    const isOnTrackOrNoDates = !isOverdue;

    // Insert separator between overdue and on-track
    if (isOnTrackOrNoDates && !insertedSeparator && insertedOverdue) {
      const sepRow = renderSeparatorRow(layout, y);
      rows.push(sepRow);
      y += sepRow.height;
      insertedSeparator = true;
    }
    if (isOverdue) insertedOverdue = true;

    const issueRow = renderSingleIssueRow(issue, node, state, layout, y, isOverdue);
    rows.push(issueRow);
    y += issueRow.height;
  }

  // "Show more" link
  if (!showAll && allSorted.length > INITIAL_ISSUE_LIMIT) {
    const remaining = allSorted.length - INITIAL_ISSUE_LIMIT;
    const showMoreRow = renderShowMoreRow(node, layout, y, allSorted.length, remaining);
    rows.push(showMoreRow);
    y += showMoreRow.height;
  }

  return rows;
}

function renderSingleIssueRow(
  issue: ChartIssue,
  parentNode: ChartNode,
  state: ScaleState,
  layout: LayoutElements,
  yOffset: number,
  isOverdue: boolean,
): RenderedRow {
  const height = getRowHeight("issue");

  // --- HTML label ---
  const row = document.createElement("div");
  row.className = `chart-row issue`;
  row.style.height = `${height}px`;
  row.dataset.issueRef = issue.issue_ref;

  const label = document.createElement("span");
  label.className = `chart-label issue-title${isOverdue ? " overdue" : ""}`;
  label.textContent = issue.title || issue.issue_ref;
  label.title = `${issue.title || ""} (${issue.issue_ref})`;
  row.appendChild(label);

  // Right-side label
  const meta = document.createElement("span");
  meta.className = "chart-badge issues";
  if (isOverdue && parentNode.end) {
    meta.textContent = formatOverdue(issue.target_date!, parentNode.end);
    meta.style.color = "#f85149";
  } else {
    const refMatch = issue.issue_ref.match(/#(\d+)$/);
    meta.textContent = refMatch ? `#${refMatch[1]}` : issue.issue_ref;
  }
  row.appendChild(meta);

  layout.labelsEl.appendChild(row);

  // --- SVG bar ---
  // Determine bar start and end, falling back to parent timeline
  const hasStart = !!issue.start_date;
  const hasTarget = !!issue.target_date;
  const barStart = issue.start_date || parentNode.start || parentNode.eff_start;
  const barEnd = issue.target_date || parentNode.end || parentNode.eff_end;
  const isAssumed = !hasStart && !hasTarget; // fully inherited from parent
  const isOpenEnded = hasStart && !hasTarget; // has start but no end

  if (barStart && barEnd) {
    const x1 = dateToX(state, barStart);
    const barY = yOffset + (height - 6) / 2;

    let x2: number;
    if (isOpenEnded) {
      // Open-ended: extend to the right edge of the visible timeline
      const range = state.currentScale.range();
      x2 = range[1];
    } else {
      x2 = dateToX(state, barEnd);
    }

    const barW = Math.max(x2 - x1, 2);

    // Main bar
    const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    rect.dataset.issueRef = issue.issue_ref;
    rect.classList.add("issue-bar");
    rect.setAttribute("x", `${x1}`);
    rect.setAttribute("y", `${barY}`);
    rect.setAttribute("width", `${barW}`);
    rect.setAttribute("height", "6");
    rect.setAttribute("rx", "2");
    rect.setAttribute("fill", "#58a6ff");
    if (isAssumed) {
      // Dimmed dashed style for assumed-from-parent timeline
      rect.setAttribute("opacity", "0.3");
      rect.setAttribute("stroke", "#58a6ff");
      rect.setAttribute("stroke-width", "1");
      rect.setAttribute("stroke-dasharray", "4,3");
      rect.setAttribute("fill", "none");
    } else if (isOpenEnded) {
      // Faded right edge for open-ended issues
      rect.setAttribute("opacity", "0.35");
    } else {
      rect.setAttribute("opacity", "0.6");
    }
    layout.barsGroup.appendChild(rect);

    // Red overdue extension (target → today)
    if (isOverdue && hasTarget) {
      const today = new Date();
      today.setHours(0, 0, 0, 0);
      const targetMs = parseDate(issue.target_date!).getTime();
      if (today.getTime() > targetMs) {
        const overdueX = dateToX(state, issue.target_date!);
        const todayX = state.currentScale(today);
        const overdueW = Math.max(todayX - overdueX, 2);

        const overdueRect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
        overdueRect.setAttribute("x", `${overdueX}`);
        overdueRect.setAttribute("y", `${barY}`);
        overdueRect.setAttribute("width", `${overdueW}`);
        overdueRect.setAttribute("height", "6");
        overdueRect.setAttribute("rx", "2");
        overdueRect.setAttribute("fill", "#f85149");
        overdueRect.setAttribute("opacity", "0.6");
        layout.barsGroup.appendChild(overdueRect);
      }
    }
  }

  return {
    type: "issue",
    issue,
    parentNode,
    labelEl: row,
    y: yOffset,
    height,
  };
}

function renderSeparatorRow(
  layout: LayoutElements,
  yOffset: number,
): RenderedRow {
  const height = getRowHeight("separator");

  const row = document.createElement("div");
  row.className = "chart-row separator";
  row.style.height = `${height}px`;
  layout.labelsEl.appendChild(row);

  // SVG dashed line
  const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
  line.setAttribute("x1", "0");
  line.setAttribute("y1", `${yOffset + height / 2}`);
  line.setAttribute("x2", "100%");
  line.setAttribute("y2", `${yOffset + height / 2}`);
  line.setAttribute("stroke", "#21262d");
  line.setAttribute("stroke-dasharray", "4,3");
  layout.barsGroup.appendChild(line);

  return { type: "separator", labelEl: row, y: yOffset, height };
}

function renderShowMoreRow(
  parentNode: ChartNode,
  layout: LayoutElements,
  yOffset: number,
  total: number,
  remaining: number,
): RenderedRow {
  const height = getRowHeight("issue");

  const row = document.createElement("div");
  row.className = "chart-row";
  row.style.height = `${height}px`;

  const link = document.createElement("span");
  link.className = "show-more-link";
  link.textContent = `▾ Show all ${total} issues (${remaining} more)`;
  row.appendChild(link);

  layout.labelsEl.appendChild(row);

  return {
    type: "show-more",
    parentNode,
    labelEl: row,
    y: yOffset,
    height,
  };
}

export { issueUrl };
