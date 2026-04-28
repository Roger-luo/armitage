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
  const diamondSize = 6;
  // At 45°, each character at ~10px font takes ~7px horizontal → ~7px diagonal.
  // With a 90px zone height, ~12 chars reach the axis tick; cap at 13 for a small margin.
  const maxChars = 13;

  const tooltip = document.getElementById("milestone-tooltip") as HTMLElement | null;

  for (const m of milestones) {
    const x = state.currentScale(parseDate(m.date));
    const isOkr = m.milestone_type === "okr";
    // CSS variable references — resolved at paint time, so they respond to theme changes.
    const colorDimVar = isOkr ? "var(--milestone-okr-dim)" : "var(--milestone-cp-dim)";
    const colorVar = isOkr ? "var(--milestone-okr)" : "var(--milestone-cp)";
    const typeLabel = isOkr ? "OKR" : "Checkpoint";

    const g = document.createElementNS("http://www.w3.org/2000/svg", "g");
    g.classList.add("milestone-line");
    g.style.cursor = "pointer";

    // Dashed vertical line — bars area only, not the label zone above
    const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
    line.setAttribute("x1", `${x}`);
    line.setAttribute("y1", `${barsTop}`);
    line.setAttribute("x2", `${x}`);
    line.setAttribute("y2", `${totalHeight}`);
    line.style.stroke = colorDimVar;
    line.style.strokeWidth = "0.8";
    line.style.strokeDasharray = "4,3";
    g.appendChild(line);

    // Diamond tick mark on the time axis
    const d = diamondSize;
    const diamond = document.createElementNS("http://www.w3.org/2000/svg", "polygon");
    diamond.setAttribute(
      "points",
      `${x},${axisHeight - d} ${x + d},${axisHeight} ${x},${axisHeight + d} ${x - d},${axisHeight}`,
    );
    diamond.style.fill = colorVar;
    diamond.style.opacity = "0.7";
    g.appendChild(diamond);

    // Transparent hit-area rect — full milestone zone width for easy hover/click
    const hitZoneWidth = 32;
    const hitRect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    hitRect.setAttribute("x", `${x - hitZoneWidth / 2}`);
    hitRect.setAttribute("y", `${axisHeight}`);
    hitRect.setAttribute("width", `${hitZoneWidth}`);
    hitRect.setAttribute("height", `${barsTop - axisHeight}`);
    hitRect.setAttribute("fill", "transparent");
    hitRect.setAttribute("pointer-events", "all");
    g.appendChild(hitRect);

    // 45° label anchored at (x, barsTop), reading left-to-right going up-left.
    // Stroke halo (paint-order: stroke fill) makes text crisp against any row background.
    const label = m.name.length > maxChars ? m.name.slice(0, maxChars - 1) + "…" : m.name;
    const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
    text.setAttribute("transform", `rotate(-45, ${x}, ${barsTop})`);
    text.setAttribute("x", `${x}`);
    text.setAttribute("y", `${barsTop}`);
    text.setAttribute("text-anchor", "end");
    text.setAttribute("dominant-baseline", "auto");
    text.style.fill = colorVar;
    text.style.stroke = "var(--bg)";
    text.style.strokeWidth = "2.5";
    text.style.paintOrder = "stroke fill";
    text.style.fontSize = "10px";
    text.style.letterSpacing = "0.01em";
    text.textContent = label;
    g.appendChild(text);

    // Hover: strengthen line + diamond, show tooltip with colored left border
    g.addEventListener("mouseover", (evt) => {
      line.style.stroke = colorVar;
      line.style.strokeWidth = "1.5";
      diamond.style.opacity = "1";
      text.style.fontWeight = "600";
      if (tooltip) {
        const typeBadgeColor = isOkr ? "var(--milestone-okr)" : "var(--milestone-cp)";
        let html = `<strong style="color:var(--text)">${m.name}</strong>`
          + `<span class="ms-type-badge" style="color:${typeBadgeColor}">${typeLabel}</span>`
          + `<br><span style="color:var(--text-muted);font-size:11px">${m.date}</span>`;
        if (m.description) {
          html += `<div style="margin-top:5px;color:var(--text-secondary);font-size:11px;line-height:1.45">${m.description}</div>`;
        }
        tooltip.innerHTML = html;
        tooltip.style.borderLeftColor = isOkr ? "var(--milestone-okr)" : "var(--milestone-cp)";
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
      line.style.stroke = colorDimVar;
      line.style.strokeWidth = "0.8";
      diamond.style.opacity = "0.7";
      text.style.fontWeight = "";
      if (tooltip) tooltip.style.display = "none";
    });
    // Click: open side panel (stop propagation to prevent bar-row selection)
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
