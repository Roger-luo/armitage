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
  if (issue.start_date && issue.target_date) {
    const x1 = dateToX(state, issue.start_date);
    const x2 = dateToX(state, issue.target_date);
    const barW = Math.max(x2 - x1, 2);
    const barY = yOffset + (height - 6) / 2;

    // Blue bar (start → target)
    const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    rect.setAttribute("x", `${x1}`);
    rect.setAttribute("y", `${barY}`);
    rect.setAttribute("width", `${barW}`);
    rect.setAttribute("height", "6");
    rect.setAttribute("rx", "2");
    rect.setAttribute("fill", "#58a6ff");
    rect.setAttribute("opacity", "0.6");
    layout.barsGroup.appendChild(rect);

    // Red overdue extension (target → today)
    if (isOverdue) {
      const today = new Date();
      today.setHours(0, 0, 0, 0);
      const targetMs = parseDate(issue.target_date).getTime();
      if (today.getTime() > targetMs) {
        const overdueX = x2;
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
