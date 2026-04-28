/**
 * Renders the time axis, grid lines, today marker, and milestone lines.
 */

import type { ScaleState } from "./scale";
import type { LayoutElements } from "./layout";
import type { ChartNode, ChartMilestone } from "./types";
import { getAxisHeight } from "./layout";
import { parseDate } from "./scale";

declare const d3: any;

export function renderAxis(
  state: ScaleState,
  layout: LayoutElements,
  totalHeight: number,
): void {
  const axisHeight = getAxisHeight();

  // Clear previous axis
  layout.axisGroup.innerHTML = "";

  // Create d3 axis
  const axis = d3
    .axisTop(state.currentScale)
    .tickSizeOuter(0)
    .tickPadding(8);

  // Render axis into the group
  const g = d3.select(layout.axisGroup)
    .attr("transform", `translate(0, ${axisHeight})`)
    .call(axis);

  // Style axis text
  g.selectAll("text")
    .attr("fill", "var(--chart-axis)")
    .attr("font-size", "11px");
  g.selectAll("line")
    .attr("stroke", "var(--chart-axis-line)");
  g.select(".domain")
    .attr("stroke", "var(--chart-axis-line)");
}

export function renderGridLines(
  state: ScaleState,
  layout: LayoutElements,
  totalHeight: number,
): void {
  layout.gridGroup.innerHTML = "";
  const ticks = state.currentScale.ticks();
  const axisHeight = getAxisHeight();

  for (const tick of ticks) {
    const x = state.currentScale(tick);
    const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
    line.setAttribute("x1", `${x}`);
    line.setAttribute("y1", `${axisHeight}`);
    line.setAttribute("x2", `${x}`);
    line.setAttribute("y2", `${totalHeight}`);
    line.setAttribute("stroke", "var(--chart-grid)");
    line.setAttribute("stroke-dasharray", "4,3");
    line.setAttribute("stroke-width", "1");
    layout.gridGroup.appendChild(line);
  }
}

export function renderTodayLine(
  state: ScaleState,
  layout: LayoutElements,
  totalHeight: number,
): void {
  // Remove all previous today line elements
  layout.markersGroup.querySelectorAll(".today-line").forEach((el) => el.remove());

  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const x = state.currentScale(today);
  const axisHeight = getAxisHeight();

  const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
  line.classList.add("today-line");
  line.setAttribute("x1", `${x}`);
  line.setAttribute("y1", `${axisHeight}`);
  line.setAttribute("x2", `${x}`);
  line.setAttribute("y2", `${totalHeight}`);
  line.setAttribute("stroke", "rgba(239, 68, 68, 0.7)");
  line.setAttribute("stroke-width", "2");
  layout.markersGroup.appendChild(line);

  // Today label
  const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
  text.classList.add("today-line");
  text.setAttribute("x", `${x}`);
  text.setAttribute("y", `${axisHeight - 4}`);
  text.setAttribute("text-anchor", "middle");
  text.setAttribute("fill", "#ef4444");
  text.setAttribute("font-size", "10px");
  text.textContent = "Today";
  layout.markersGroup.appendChild(text);
}

export function renderMilestoneLines(
  state: ScaleState,
  layout: LayoutElements,
  totalHeight: number,
  milestones: ChartMilestone[],
): void {
  layout.markersGroup.querySelectorAll(".milestone-line").forEach((el) => el.remove());
  const axisHeight = getAxisHeight();

  // Small diamond marker height above axis
  const diamondSize = 5;
  // Labels run vertically downward along the milestone line, starting just below the axis.
  // This means arbitrarily close milestones never overlap — each label lives in its own column.
  const labelOffsetFromAxis = 8;
  const fontSize = 10;

  for (const m of milestones) {
    const x = state.currentScale(parseDate(m.date));
    const isOkr = m.milestone_type === "okr";
    const color = isOkr ? "rgba(167, 139, 250, 0.5)" : "rgba(245, 158, 11, 0.5)";
    const labelColor = isOkr ? "#a78bfa" : "#f59e0b";

    // Dashed vertical line through the chart body
    const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
    line.classList.add("milestone-line");
    line.setAttribute("x1", `${x}`);
    line.setAttribute("y1", `${axisHeight}`);
    line.setAttribute("x2", `${x}`);
    line.setAttribute("y2", `${totalHeight}`);
    line.setAttribute("stroke", color);
    line.setAttribute("stroke-width", "1");
    line.setAttribute("stroke-dasharray", "4,3");
    layout.markersGroup.appendChild(line);

    // Small diamond on the axis line to mark the milestone date
    const diamond = document.createElementNS("http://www.w3.org/2000/svg", "polygon");
    diamond.classList.add("milestone-line");
    const d = diamondSize;
    diamond.setAttribute(
      "points",
      `${x},${axisHeight - d} ${x + d},${axisHeight} ${x},${axisHeight + d} ${x - d},${axisHeight}`,
    );
    diamond.setAttribute("fill", labelColor);
    diamond.setAttribute("opacity", "0.85");
    layout.markersGroup.appendChild(diamond);

    // Label rotated 90° CW (reads top-to-bottom) running downward along the milestone line.
    // After rotate(90, x, y): local +x → global +y (down), local -y (above baseline) → global +x (right).
    // So with text-anchor:start and default baseline, characters sit to the RIGHT of x in global space.
    // dx shifts the text start downward (local +x = global +y), creating a gap below the axis.
    const labelY = axisHeight + labelOffsetFromAxis;
    const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
    text.classList.add("milestone-line");
    text.setAttribute("transform", `rotate(90, ${x}, ${labelY})`);
    text.setAttribute("x", `${x}`);
    text.setAttribute("y", `${labelY}`);
    text.setAttribute("text-anchor", "start");
    text.setAttribute("fill", labelColor);
    text.setAttribute("font-size", `${fontSize}px`);
    const title = document.createElementNS("http://www.w3.org/2000/svg", "title");
    title.textContent = `${m.name} (${m.date})`;
    text.appendChild(title);
    text.appendChild(document.createTextNode(m.name));
    layout.markersGroup.appendChild(text);
  }
}
