/**
 * Time scale and zoom management using d3-scale and d3-zoom.
 */

import type { LayoutElements } from "./layout";

// D3 is loaded from CDN as a global
declare const d3: any;

export interface ScaleState {
  baseScale: any; // d3.ScaleTime
  currentScale: any; // d3.ScaleTime (after zoom transform)
  zoom: any; // d3.ZoomBehavior
  transform: any; // d3.ZoomTransform
}

export function createScale(
  domain: [Date, Date],
  rangeWidth: number,
): ScaleState {
  const baseScale = d3
    .scaleTime()
    .domain(domain)
    .range([0, rangeWidth]);

  return {
    baseScale,
    currentScale: baseScale.copy(),
    zoom: null,
    transform: d3.zoomIdentity,
  };
}

export function setupZoom(
  state: ScaleState,
  layout: LayoutElements,
  onZoom: (newScale: any) => void,
): void {
  const pad = 30 * 24 * 3600 * 1000;
  const [domainStart, domainEnd] = state.baseScale.domain();
  const rangeWidth = state.baseScale.range()[1];

  state.zoom = d3
    .zoom()
    .scaleExtent([0.5, 50])
    .translateExtent([
      [state.baseScale(domainStart.getTime() - pad), 0],
      [state.baseScale(domainEnd.getTime() + pad), 0],
    ])
    .filter((event: any) => {
      // Allow wheel, touch, and mouse events but not double-click (used for drill-down)
      return !event.type.startsWith("dblclick");
    })
    .on("zoom", (event: any) => {
      state.transform = event.transform;
      // Constrain to horizontal only: reset y translation
      state.transform = d3.zoomIdentity
        .translate(event.transform.x, 0)
        .scale(event.transform.k);
      state.currentScale = state.transform.rescaleX(state.baseScale);
      onZoom(state.currentScale);
    });

  d3.select(layout.timelineSvg).call(state.zoom);
}

export function resetZoom(
  state: ScaleState,
  layout: LayoutElements,
  newDomain?: [Date, Date],
): void {
  if (newDomain) {
    state.baseScale.domain(newDomain);
  }
  state.transform = d3.zoomIdentity;
  state.currentScale = state.baseScale.copy();
  d3.select(layout.timelineSvg)
    .call(state.zoom.transform, d3.zoomIdentity);
}

export function updateScaleRange(state: ScaleState, rangeWidth: number): void {
  state.baseScale.range([0, rangeWidth]);
  state.currentScale = state.transform.rescaleX(state.baseScale);
}

/** Convert a date string "YYYY-MM-DD" to a Date object. */
export function parseDate(s: string): Date {
  return new Date(s + "T00:00:00");
}

/** Get pixel x for a date string using the current (zoomed) scale. */
export function dateToX(state: ScaleState, dateStr: string): number {
  return state.currentScale(parseDate(dateStr));
}
