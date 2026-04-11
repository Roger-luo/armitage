/** Mirrors the Rust ChartIssue struct. */
export interface ChartIssue {
  issue_ref: string;
  title: string | null;
  start_date: string | null;
  target_date: string | null;
  state: string | null;
  description: string | null;
  labels: string[];
  author: string | null;
}

/** Mirrors the Rust ChartMilestone struct. */
export interface ChartMilestone {
  name: string;
  date: string;
  description: string;
  milestone_type: "checkpoint" | "okr";
}

/** Mirrors the Rust ChartNode struct. */
export interface ChartNode {
  path: string;
  name: string;
  description: string;
  status: "active" | "completed" | "paused" | "cancelled";
  start: string | null;
  end: string | null;
  eff_start: string | null;
  eff_end: string | null;
  has_timeline: boolean;
  owners: string[];
  team: string | null;
  overflow_end: string | null;
  children: ChartNode[];
  milestones: ChartMilestone[];
  issues: ChartIssue[];
}

/** Top-level chart data injected into window.__CHART_DATA__. */
export interface ChartData {
  nodes: ChartNode[];
  org_name: string;
  global_start: string | null;
  global_end: string | null;
}

declare global {
  interface Window {
    __CHART_DATA__: ChartData;
  }
}
