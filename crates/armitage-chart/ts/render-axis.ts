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
  barsTop: number,
): void {
  layout.markersGroup.querySelectorAll(".milestone-line").forEach((el) => el.remove());
  const axisHeight = getAxisHeight();
  const diamondSize = 5;
  const fontSize = 9;
  // At 45°, the zone height (barsTop - axisHeight) in px corresponds to the same diagonal length.
  // Empirically ~7px per char at 9px font, so 90px zone ≈ 12 chars before hitting the axis.
  const maxChars = 14;

  const tooltip = document.getElementById("milestone-tooltip");

  for (const m of milestones) {
    const x = state.currentScale(parseDate(m.date));
    const isOkr = m.milestone_type === "okr";
    const colorDim = isOkr ? "rgba(167, 139, 250, 0.5)" : "rgba(245, 158, 11, 0.5)";
    const colorBright = isOkr ? "rgba(167, 139, 250, 0.9)" : "rgba(245, 158, 11, 0.9)";
    const labelColor = isOkr ? "#a78bfa" : "#f59e0b";

    const g = document.createElementNS("http://www.w3.org/2000/svg", "g");
    g.classList.add("milestone-line");
    g.style.cursor = "pointer";

    // Dashed vertical line through the bars area only (not the label zone)
    const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
    line.setAttribute("x1", `${x}`);
    line.setAttribute("y1", `${barsTop}`);
    line.setAttribute("x2", `${x}`);
    line.setAttribute("y2", `${totalHeight}`);
    line.setAttribute("stroke", colorDim);
    line.setAttribute("stroke-width", "1");
    line.setAttribute("stroke-dasharray", "4,3");
    g.appendChild(line);

    // Diamond marker on the time axis
    const diamond = document.createElementNS("http://www.w3.org/2000/svg", "polygon");
    const d = diamondSize;
    diamond.setAttribute(
      "points",
      `${x},${axisHeight - d} ${x + d},${axisHeight} ${x},${axisHeight + d} ${x - d},${axisHeight}`,
    );
    diamond.setAttribute("fill", labelColor);
    diamond.setAttribute("opacity", "0.85");
    g.appendChild(diamond);

    // Transparent hit-area rect covering the diagonal label zone for easier mouse interaction.
    // Centered on x, spanning from axisHeight to barsTop vertically.
    const hitZoneWidth = 32;
    const hitRect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    hitRect.setAttribute("x", `${x - hitZoneWidth / 2}`);
    hitRect.setAttribute("y", `${axisHeight}`);
    hitRect.setAttribute("width", `${hitZoneWidth}`);
    hitRect.setAttribute("height", `${barsTop - axisHeight}`);
    hitRect.setAttribute("fill", "transparent");
    hitRect.setAttribute("pointer-events", "all");
    g.appendChild(hitRect);

    // 45° label anchored at (x, barsTop), extending up-left into the milestone zone.
    // rotate(-45) tilts the text so it reads left-to-right going from lower-right to upper-left.
    // text-anchor="end" places the end of the text at the anchor point.
    const label = m.name.length > maxChars ? m.name.slice(0, maxChars - 1) + "…" : m.name;
    const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
    text.setAttribute("transform", `rotate(-45, ${x}, ${barsTop})`);
    text.setAttribute("x", `${x}`);
    text.setAttribute("y", `${barsTop}`);
    text.setAttribute("text-anchor", "end");
    text.setAttribute("dominant-baseline", "auto");
    text.setAttribute("fill", labelColor);
    text.setAttribute("font-size", `${fontSize}px`);
    text.textContent = label;
    g.appendChild(text);

    // Hover: brighten line + show tooltip
    g.addEventListener("mouseover", (evt) => {
      line.setAttribute("stroke", colorBright);
      line.setAttribute("stroke-width", "2");
      diamond.setAttribute("opacity", "1");
      text.setAttribute("font-weight", "bold");
      if (tooltip) {
        let html = `<strong>${m.name}</strong>&nbsp;<span style="color:var(--text-muted);font-size:11px">${m.date}</span>`;
        if (m.description) html += `<br><span style="color:var(--text-secondary)">${m.description}</span>`;
        tooltip.innerHTML = html;
        tooltip.style.display = "block";
        const me = evt as MouseEvent;
        tooltip.style.left = `${me.clientX + 14}px`;
        tooltip.style.top = `${me.clientY - 8}px`;
      }
    });
    g.addEventListener("mousemove", (evt) => {
      if (tooltip) {
        const me = evt as MouseEvent;
        tooltip.style.left = `${me.clientX + 14}px`;
        tooltip.style.top = `${me.clientY - 8}px`;
      }
    });
    g.addEventListener("mouseout", () => {
      line.setAttribute("stroke", colorDim);
      line.setAttribute("stroke-width", "1");
      diamond.setAttribute("opacity", "0.85");
      text.removeAttribute("font-weight");
      if (tooltip) tooltip.style.display = "none";
    });
    // Click: open side panel with milestone details (stop propagation to prevent row clicks)
    g.addEventListener("click", (evt) => {
      evt.stopPropagation();
      if (tooltip) tooltip.style.display = "none";
      if ((window as any).__openMilestonePanel) {
        (window as any).__openMilestonePanel(m);
      }
    });

    layout.markersGroup.appendChild(g);
  }
}
