/**
 * Split-panel layout management.
 * Creates and manages the label column (HTML) and timeline column (SVG).
 */

const LABEL_WIDTH = 200;
const NODE_ROW_HEIGHT = 48;
const ISSUE_ROW_HEIGHT = 28;
const SEPARATOR_HEIGHT = 12;
const AXIS_HEIGHT = 40;
// Extra height between the time axis and the first bar row, reserved for 45° milestone labels.
// Set to 0 when there are no milestones to render.
export const MILESTONE_ZONE_HEIGHT = 90;

export interface LayoutElements {
  labelsEl: HTMLDivElement;
  timelineSvg: SVGSVGElement;
  axisGroup: SVGGElement;
  gridGroup: SVGGElement;
  barsGroup: SVGGElement;
  markersGroup: SVGGElement;
  scrollEl: HTMLDivElement;
}

export function getLayoutElements(): LayoutElements {
  return {
    labelsEl: document.getElementById("chart-labels") as HTMLDivElement,
    timelineSvg: document.getElementById("chart-svg") as unknown as SVGSVGElement,
    axisGroup: document.getElementById("axis-group") as unknown as SVGGElement,
    gridGroup: document.getElementById("grid-group") as unknown as SVGGElement,
    barsGroup: document.getElementById("bars-group") as unknown as SVGGElement,
    markersGroup: document.getElementById("markers-group") as unknown as SVGGElement,
    scrollEl: document.getElementById("chart-scroll") as HTMLDivElement,
  };
}

export function getTimelineWidth(): number {
  const el = document.getElementById("chart-timeline");
  return el ? el.clientWidth : 800;
}

export function getRowHeight(type: "node" | "issue" | "separator"): number {
  if (type === "issue") return ISSUE_ROW_HEIGHT;
  if (type === "separator") return SEPARATOR_HEIGHT;
  return NODE_ROW_HEIGHT;
}

export function getAxisHeight(): number {
  return AXIS_HEIGHT;
}

/** y-coordinate where bars begin (after axis + optional milestone zone). */
export function getBarsTop(hasMilestones: boolean): number {
  return AXIS_HEIGHT + (hasMilestones ? MILESTONE_ZONE_HEIGHT : 0);
}

/** Sync the SVG height and bar group position. */
export function syncSvgHeight(layout: LayoutElements, totalRowHeight: number, barsTop: number): void {
  const totalHeight = totalRowHeight + barsTop;
  layout.timelineSvg.setAttribute("height", `${totalHeight}`);
  layout.barsGroup.setAttribute("transform", `translate(0, ${barsTop})`);
}
