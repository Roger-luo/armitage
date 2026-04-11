"use strict";
(() => {
  // ts/layout.ts
  var NODE_ROW_HEIGHT = 48;
  var ISSUE_ROW_HEIGHT = 28;
  var SEPARATOR_HEIGHT = 12;
  var AXIS_HEIGHT = 40;
  function getLayoutElements() {
    return {
      labelsEl: document.getElementById("chart-labels"),
      timelineSvg: document.getElementById("chart-svg"),
      axisGroup: document.getElementById("axis-group"),
      gridGroup: document.getElementById("grid-group"),
      barsGroup: document.getElementById("bars-group"),
      markersGroup: document.getElementById("markers-group"),
      scrollEl: document.getElementById("chart-scroll")
    };
  }
  function getTimelineWidth() {
    const el = document.getElementById("chart-timeline");
    return el ? el.clientWidth : 800;
  }
  function getRowHeight(type) {
    if (type === "issue") return ISSUE_ROW_HEIGHT;
    if (type === "separator") return SEPARATOR_HEIGHT;
    return NODE_ROW_HEIGHT;
  }
  function getAxisHeight() {
    return AXIS_HEIGHT;
  }
  function syncSvgHeight(layout2, totalRowHeight) {
    const totalHeight = totalRowHeight + AXIS_HEIGHT;
    layout2.timelineSvg.setAttribute("height", `${totalHeight}`);
    layout2.barsGroup.setAttribute("transform", `translate(0, ${AXIS_HEIGHT})`);
  }

  // ts/scale.ts
  function createScale(domain, rangeWidth) {
    const baseScale = d3.scaleTime().domain(domain).range([0, rangeWidth]);
    return {
      baseScale,
      currentScale: baseScale.copy(),
      zoom: null,
      transform: d3.zoomIdentity
    };
  }
  function setupZoom(state, layout2, onZoom2) {
    const pad = 30 * 24 * 3600 * 1e3;
    const [domainStart, domainEnd] = state.baseScale.domain();
    const rangeWidth = state.baseScale.range()[1];
    state.zoom = d3.zoom().scaleExtent([0.5, 50]).translateExtent([
      [state.baseScale(domainStart.getTime() - pad), 0],
      [state.baseScale(domainEnd.getTime() + pad), 0]
    ]).filter((event) => {
      return !event.type.startsWith("dblclick");
    }).on("zoom", (event) => {
      state.transform = event.transform;
      state.transform = d3.zoomIdentity.translate(event.transform.x, 0).scale(event.transform.k);
      state.currentScale = state.transform.rescaleX(state.baseScale);
      onZoom2(state.currentScale);
    });
    d3.select(layout2.timelineSvg).call(state.zoom);
  }
  function resetZoom(state, layout2, newDomain) {
    if (newDomain) {
      state.baseScale.domain(newDomain);
    }
    state.transform = d3.zoomIdentity;
    state.currentScale = state.baseScale.copy();
    d3.select(layout2.timelineSvg).call(state.zoom.transform, d3.zoomIdentity);
  }
  function updateScaleRange(state, rangeWidth) {
    state.baseScale.range([0, rangeWidth]);
    state.currentScale = state.transform.rescaleX(state.baseScale);
  }
  function parseDate(s) {
    return /* @__PURE__ */ new Date(s + "T00:00:00");
  }
  function dateToX(state, dateStr) {
    return state.currentScale(parseDate(dateStr));
  }

  // ts/render-axis.ts
  function renderAxis(state, layout2, totalHeight) {
    const axisHeight = getAxisHeight();
    layout2.axisGroup.innerHTML = "";
    const axis = d3.axisTop(state.currentScale).tickSizeOuter(0).tickPadding(8);
    const g = d3.select(layout2.axisGroup).attr("transform", `translate(0, ${axisHeight})`).call(axis);
    g.selectAll("text").attr("fill", "var(--chart-axis)").attr("font-size", "11px");
    g.selectAll("line").attr("stroke", "var(--chart-axis-line)");
    g.select(".domain").attr("stroke", "var(--chart-axis-line)");
  }
  function renderGridLines(state, layout2, totalHeight) {
    layout2.gridGroup.innerHTML = "";
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
      layout2.gridGroup.appendChild(line);
    }
  }
  function renderTodayLine(state, layout2, totalHeight) {
    layout2.markersGroup.querySelectorAll(".today-line").forEach((el) => el.remove());
    const today = /* @__PURE__ */ new Date();
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
    layout2.markersGroup.appendChild(line);
    const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
    text.classList.add("today-line");
    text.setAttribute("x", `${x}`);
    text.setAttribute("y", `${axisHeight - 4}`);
    text.setAttribute("text-anchor", "middle");
    text.setAttribute("fill", "#ef4444");
    text.setAttribute("font-size", "10px");
    text.textContent = "Today";
    layout2.markersGroup.appendChild(text);
  }
  function renderMilestoneLines(state, layout2, totalHeight, milestones) {
    layout2.markersGroup.querySelectorAll(".milestone-line").forEach((el) => el.remove());
    const axisHeight = getAxisHeight();
    for (const m of milestones) {
      const x = state.currentScale(parseDate(m.date));
      const isOkr = m.milestone_type === "okr";
      const color = isOkr ? "rgba(167, 139, 250, 0.5)" : "rgba(245, 158, 11, 0.5)";
      const labelColor = isOkr ? "#a78bfa" : "#f59e0b";
      const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
      line.classList.add("milestone-line");
      line.setAttribute("x1", `${x}`);
      line.setAttribute("y1", `${axisHeight}`);
      line.setAttribute("x2", `${x}`);
      line.setAttribute("y2", `${totalHeight}`);
      line.setAttribute("stroke", color);
      line.setAttribute("stroke-width", "1");
      line.setAttribute("stroke-dasharray", "4,3");
      layout2.markersGroup.appendChild(line);
      const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
      text.classList.add("milestone-line");
      text.setAttribute("x", `${x}`);
      text.setAttribute("y", `${axisHeight - 4}`);
      text.setAttribute("text-anchor", "middle");
      text.setAttribute("fill", labelColor);
      text.setAttribute("font-size", "10px");
      text.textContent = m.name;
      layout2.markersGroup.appendChild(text);
    }
  }

  // ts/render-nodes.ts
  function resolveTimeline(node, ancestors) {
    if (node.start && node.end) return { start: node.start, end: node.end };
    if (node.eff_start && node.eff_end) return { start: node.eff_start, end: node.eff_end };
    for (const ancestor of ancestors) {
      if (ancestor.start && ancestor.end) return { start: ancestor.start, end: ancestor.end };
      if (ancestor.eff_start && ancestor.eff_end) return { start: ancestor.eff_start, end: ancestor.eff_end };
    }
    return null;
  }
  var STATUS_COLORS = {
    active: "#3b82f6",
    completed: "#6b7280",
    paused: "#f59e0b",
    cancelled: "#ef4444"
  };
  function sortIssues(issues, nodeEnd) {
    const overdue = [];
    const onTrack = [];
    const noDates = [];
    for (const issue of issues) {
      if (!issue.target_date) {
        noDates.push(issue);
      } else if (nodeEnd && issue.target_date > nodeEnd) {
        overdue.push(issue);
      } else {
        onTrack.push(issue);
      }
    }
    overdue.sort((a, b) => b.target_date.localeCompare(a.target_date));
    onTrack.sort((a, b) => a.target_date.localeCompare(b.target_date));
    return { overdue, onTrack, noDates };
  }
  function renderNodeRow(node, state, layout2, yOffset, options) {
    const height = getRowHeight("node");
    const row = document.createElement("div");
    row.className = `chart-row node${options.isDimmed ? " dimmed" : ""}${options.isExpanded ? " expanded" : ""}`;
    row.style.height = `${height}px`;
    row.dataset.path = node.path;
    const label = document.createElement("span");
    label.className = "chart-label node-name";
    label.textContent = node.name;
    label.title = node.description || node.name;
    row.appendChild(label);
    if (node.children.length > 0) {
      const badge = document.createElement("span");
      badge.className = "chart-badge children";
      badge.textContent = `\xD7${node.children.length}`;
      row.appendChild(badge);
      const arrow = document.createElement("span");
      arrow.className = "chart-drill";
      arrow.textContent = "\u25B8";
      row.appendChild(arrow);
    } else if (node.issues.length > 0) {
      const badge = document.createElement("span");
      badge.className = "chart-badge issues";
      const overdueCount = node.issues.filter(
        (i) => i.target_date && node.end && i.target_date > node.end
      ).length;
      if (overdueCount > 0) {
        badge.innerHTML = `${node.issues.length} issues \xB7 <span class="overdue-count">${overdueCount} overdue</span>`;
      } else {
        badge.textContent = `${node.issues.length} issues`;
      }
      row.appendChild(badge);
    }
    layout2.labelsEl.appendChild(row);
    const statusColor = STATUS_COLORS[node.status] || STATUS_COLORS.active;
    const barY = yOffset;
    const ancestors = options.parentNode ? [options.parentNode] : [];
    const timeline = resolveTimeline(node, ancestors);
    const barStart = timeline?.start;
    const barEnd = timeline?.end;
    const isInherited = !node.eff_start && !node.eff_end && !!barStart;
    if (barStart && barEnd) {
      const x1 = dateToX(state, barStart);
      const x2 = dateToX(state, barEnd);
      const barW = Math.max(x2 - x1, 2);
      const barH = height - 8;
      const barTop = barY + 4;
      const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
      rect.setAttribute("x", `${x1}`);
      rect.setAttribute("y", `${barTop}`);
      rect.setAttribute("width", `${barW}`);
      rect.setAttribute("height", `${barH}`);
      rect.setAttribute("rx", "4");
      if (isInherited) {
        rect.setAttribute("fill", "rgba(107,114,128,0.08)");
        rect.setAttribute("stroke", "rgba(107,114,128,0.3)");
        rect.setAttribute("stroke-dasharray", "4,3");
      } else {
        rect.setAttribute("fill", node.has_timeline ? `${statusColor}22` : "rgba(107,114,128,0.15)");
        rect.setAttribute("stroke", options.isExpanded ? "rgba(88,166,255,0.6)" : node.has_timeline ? `${statusColor}55` : "rgba(107,114,128,0.4)");
        if (!node.has_timeline && !options.isExpanded) {
          rect.setAttribute("stroke-dasharray", "4,3");
        }
      }
      rect.setAttribute("stroke-width", options.isExpanded ? "2" : "1");
      if (options.isDimmed) rect.setAttribute("opacity", "0.4");
      rect.dataset.path = node.path;
      rect.classList.add("node-bar");
      layout2.barsGroup.appendChild(rect);
      if (node.children.length > 0) {
        const childrenWithTimeline = node.children.filter((c) => c.eff_start && c.eff_end);
        if (childrenWithTimeline.length > 0) {
          const spanStart = childrenWithTimeline.reduce((min, c) => c.eff_start < min ? c.eff_start : min, childrenWithTimeline[0].eff_start);
          const spanEnd = childrenWithTimeline.reduce((max, c) => c.eff_end > max ? c.eff_end : max, childrenWithTimeline[0].eff_end);
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
          layout2.barsGroup.appendChild(fillRect);
        }
      }
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
          layout2.barsGroup.appendChild(tick);
        }
      }
    }
    return { type: "node", node, labelEl: row, y: yOffset, height };
  }
  function formatOverdue(targetDate, nodeEnd) {
    const target = parseDate(targetDate).getTime();
    const end = parseDate(nodeEnd).getTime();
    const diffMs = target - end;
    if (diffMs <= 0) return "";
    const diffDays = Math.ceil(diffMs / (24 * 3600 * 1e3));
    if (diffDays < 14) return `+${diffDays} days`;
    const diffWeeks = Math.round(diffDays / 7);
    return `+${diffWeeks} wks`;
  }

  // ts/render-issues.ts
  var INITIAL_ISSUE_LIMIT = 7;
  function issueUrl(ref) {
    const match = ref.match(/^(.+?)\/(.+?)#(\d+)$/);
    if (!match) return "#";
    return `https://github.com/${match[1]}/${match[2]}/issues/${match[3]}`;
  }
  function renderIssueRows(node, state, layout2, yOffset, showAll, ancestors = []) {
    const rows = [];
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
      if (isOnTrackOrNoDates && !insertedSeparator && insertedOverdue) {
        const sepRow = renderSeparatorRow(layout2, y);
        rows.push(sepRow);
        y += sepRow.height;
        insertedSeparator = true;
      }
      if (isOverdue) insertedOverdue = true;
      const issueRow = renderSingleIssueRow(issue, node, state, layout2, y, isOverdue, ancestors);
      rows.push(issueRow);
      y += issueRow.height;
    }
    if (!showAll && allSorted.length > INITIAL_ISSUE_LIMIT) {
      const remaining = allSorted.length - INITIAL_ISSUE_LIMIT;
      const showMoreRow = renderShowMoreRow(node, layout2, y, allSorted.length, remaining);
      rows.push(showMoreRow);
      y += showMoreRow.height;
    }
    return rows;
  }
  function renderSingleIssueRow(issue, parentNode, state, layout2, yOffset, isOverdue, ancestors = []) {
    const height = getRowHeight("issue");
    const row = document.createElement("div");
    row.className = `chart-row issue`;
    row.style.height = `${height}px`;
    row.dataset.issueRef = issue.issue_ref;
    const label = document.createElement("span");
    label.className = `chart-label issue-title${isOverdue ? " overdue" : ""}`;
    label.textContent = issue.title || issue.issue_ref;
    label.title = `${issue.title || ""} (${issue.issue_ref})`;
    row.appendChild(label);
    const meta = document.createElement("span");
    meta.className = "chart-badge issues";
    if (isOverdue && parentNode.end) {
      meta.textContent = formatOverdue(issue.target_date, parentNode.end);
      meta.style.color = "#f85149";
    } else {
      const refMatch = issue.issue_ref.match(/#(\d+)$/);
      meta.textContent = refMatch ? `#${refMatch[1]}` : issue.issue_ref;
    }
    row.appendChild(meta);
    layout2.labelsEl.appendChild(row);
    const hasStart = !!issue.start_date;
    const hasTarget = !!issue.target_date;
    const inherited = resolveTimeline(parentNode, ancestors);
    const barStart = issue.start_date || inherited?.start;
    const barEnd = issue.target_date || inherited?.end;
    const isAssumed = !hasStart && !hasTarget;
    const isOpenEnded = hasStart && !hasTarget;
    if (barStart && barEnd) {
      const x1 = dateToX(state, barStart);
      const barY = yOffset + (height - 6) / 2;
      let x2;
      if (isOpenEnded) {
        const range = state.currentScale.range();
        x2 = range[1];
      } else {
        x2 = dateToX(state, barEnd);
      }
      const barW = Math.max(x2 - x1, 2);
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
        rect.setAttribute("opacity", "0.3");
        rect.setAttribute("stroke", "#58a6ff");
        rect.setAttribute("stroke-width", "1");
        rect.setAttribute("stroke-dasharray", "4,3");
        rect.setAttribute("fill", "none");
      } else if (isOpenEnded) {
        rect.setAttribute("opacity", "0.35");
      } else {
        rect.setAttribute("opacity", "0.6");
      }
      layout2.barsGroup.appendChild(rect);
      if (isOverdue && hasTarget) {
        const today = /* @__PURE__ */ new Date();
        today.setHours(0, 0, 0, 0);
        const targetMs = parseDate(issue.target_date).getTime();
        if (today.getTime() > targetMs) {
          const overdueX = dateToX(state, issue.target_date);
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
          layout2.barsGroup.appendChild(overdueRect);
        }
      }
    }
    return {
      type: "issue",
      issue,
      parentNode,
      labelEl: row,
      y: yOffset,
      height
    };
  }
  function renderSeparatorRow(layout2, yOffset) {
    const height = getRowHeight("separator");
    const row = document.createElement("div");
    row.className = "chart-row separator";
    row.style.height = `${height}px`;
    layout2.labelsEl.appendChild(row);
    const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
    line.setAttribute("x1", "0");
    line.setAttribute("y1", `${yOffset + height / 2}`);
    line.setAttribute("x2", "100%");
    line.setAttribute("y2", `${yOffset + height / 2}`);
    line.setAttribute("stroke", "#21262d");
    line.setAttribute("stroke-dasharray", "4,3");
    layout2.barsGroup.appendChild(line);
    return { type: "separator", labelEl: row, y: yOffset, height };
  }
  function renderShowMoreRow(parentNode, layout2, yOffset, total, remaining) {
    const height = getRowHeight("issue");
    const row = document.createElement("div");
    row.className = "chart-row";
    row.style.height = `${height}px`;
    const link = document.createElement("span");
    link.className = "show-more-link";
    link.textContent = `\u25BE Show all ${total} issues (${remaining} more)`;
    row.appendChild(link);
    layout2.labelsEl.appendChild(row);
    return {
      type: "show-more",
      parentNode,
      labelEl: row,
      y: yOffset,
      height
    };
  }

  // ts/chart.ts
  var data = window.__CHART_DATA__;
  var currentPath = "";
  var useGlobalRange = false;
  var selectedNode = null;
  var expandedNode = null;
  var expandedShowAll = false;
  var layout;
  var scaleState;
  var renderedRows = [];
  function getVisibleNodes() {
    if (currentPath === "") return data.nodes;
    const node = findNode(data.nodes, currentPath);
    return node ? node.children : [];
  }
  function findNode(nodes, path) {
    for (const n of nodes) {
      if (n.path === path) return n;
      const found = findNode(n.children, path);
      if (found) return found;
    }
    return null;
  }
  function collectOkrs(nodes) {
    const seen = /* @__PURE__ */ new Set();
    const result = [];
    function walk(ns) {
      for (const n of ns) {
        for (const m of n.milestones) {
          if (m.milestone_type === "okr") {
            const key = `${m.name}|${m.date}`;
            if (!seen.has(key)) {
              seen.add(key);
              result.push(m);
            }
          }
        }
        walk(n.children);
      }
    }
    walk(nodes);
    return result;
  }
  function allCheckpoints(node) {
    const result = [];
    function walk(n) {
      for (const m of n.milestones) {
        if (m.milestone_type !== "okr") result.push(m);
      }
      for (const c of n.children) walk(c);
    }
    walk(node);
    return result;
  }
  function computeTimeRange(nodes) {
    if (useGlobalRange && data.global_start && data.global_end) {
      return [parseDate(data.global_start), parseDate(data.global_end)];
    }
    let min = Infinity;
    let max = -Infinity;
    for (const n of nodes) {
      if (n.eff_start) min = Math.min(min, parseDate(n.eff_start).getTime());
      if (n.eff_end) max = Math.max(max, parseDate(n.eff_end).getTime());
    }
    if (expandedNode) {
      const expNode = nodes.find((n) => n.path === expandedNode);
      if (expNode) {
        for (const issue of expNode.issues) {
          if (issue.start_date) min = Math.min(min, parseDate(issue.start_date).getTime());
          if (issue.target_date) max = Math.max(max, parseDate(issue.target_date).getTime());
        }
        if (expNode.overflow_end) {
          max = Math.max(max, (/* @__PURE__ */ new Date()).setHours(0, 0, 0, 0));
        }
      }
    }
    if (min === Infinity || max === -Infinity) {
      const now = /* @__PURE__ */ new Date();
      min = new Date(now.getFullYear(), 0, 1).getTime();
      max = new Date(now.getFullYear(), 11, 31).getTime();
    }
    const pad = 30 * 24 * 3600 * 1e3;
    return [new Date(min - pad), new Date(max + pad)];
  }
  function escapeHtml(s) {
    const div = document.createElement("div");
    div.textContent = s;
    return div.innerHTML;
  }
  var panelEl = document.getElementById("panel");
  var panelContentEl = document.getElementById("panel-content");
  function showNodePanel(node) {
    selectedNode = node;
    let html = "";
    html += `<h2>${escapeHtml(node.name)}</h2>`;
    html += `<span class="panel-status ${node.status}">${node.status}</span>`;
    if (node.description) {
      html += `<div class="panel-section"><h3>Description</h3><div class="panel-desc">${escapeHtml(node.description)}</div></div>`;
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
        const url = issueUrl(issue.issue_ref);
        const label = issue.title ? `${escapeHtml(issue.title)} <span class="issue-ref">${escapeHtml(issue.issue_ref)}</span>` : escapeHtml(issue.issue_ref);
        html += `<li><a class="panel-issue-link" href="${url}" target="_blank" rel="noopener">${label}</a></li>`;
      }
      html += `</ul></div>`;
    }
    panelContentEl.innerHTML = html;
    panelEl.classList.add("open");
  }
  function showIssuePanel(issue, parentNode) {
    selectedNode = null;
    const url = issueUrl(issue.issue_ref);
    const isOverdue = issue.target_date && parentNode.end && issue.target_date > parentNode.end;
    let html = "";
    html += `<h2>${escapeHtml(issue.title || issue.issue_ref)}</h2>`;
    html += `<a class="panel-issue-link" href="${url}" target="_blank" rel="noopener">${escapeHtml(issue.issue_ref)} &rarr; Open on GitHub</a>`;
    html += `<span class="panel-status ${issue.state === "CLOSED" ? "completed" : "active"}">${(issue.state || "OPEN").toLowerCase()}</span>`;
    if (issue.author) {
      html += `<div class="panel-section"><h3>Author</h3>`;
      html += `<div class="panel-meta"><a class="panel-author-link" href="https://github.com/${encodeURIComponent(issue.author)}" target="_blank" rel="noopener">@${escapeHtml(issue.author)}</a></div>`;
      html += `</div>`;
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
      html += `<br/><span class="issue-overflow">Overdue: ${formatOverdue(issue.target_date, parentNode.end)} past ${escapeHtml(parentNode.name)} deadline</span>`;
    }
    html += `</div></div>`;
    html += `<div class="panel-section"><h3>Parent</h3>`;
    html += `<div class="panel-meta"><span class="crumb" onclick="window.__nav('${parentNode.path}')">${escapeHtml(parentNode.name)}</span></div></div>`;
    if (issue.description) {
      html += `<div class="panel-section"><h3>Description</h3>`;
      html += `<div class="panel-desc">${escapeHtml(issue.description)}</div>`;
      html += `</div>`;
    }
    panelContentEl.innerHTML = html;
    panelEl.classList.add("open");
  }
  function closePanel() {
    selectedNode = null;
    panelEl.classList.remove("open");
  }
  window.__closePanel = closePanel;
  var breadcrumbEl = document.getElementById("breadcrumb");
  function updateBreadcrumb() {
    const parts = [
      { label: data.org_name || "Root", path: "" }
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
    breadcrumbEl.innerHTML = parts.map((p, i) => {
      if (i === parts.length - 1) {
        return `<span class="crumb-current">${p.label}</span>`;
      }
      return `<span class="crumb" onclick="window.__nav('${p.path}')">${p.label}</span>`;
    }).join('<span class="crumb-sep"> &rsaquo; </span>');
  }
  function renderChart() {
    const nodes = getVisibleNodes();
    const timeRange = computeTimeRange(nodes);
    const timelineWidth = getTimelineWidth();
    scaleState.baseScale.domain(timeRange).range([0, timelineWidth]);
    scaleState.currentScale = scaleState.transform.rescaleX(scaleState.baseScale);
    layout.labelsEl.innerHTML = "";
    layout.barsGroup.innerHTML = "";
    renderedRows = [];
    const ancestors = [];
    if (currentPath) {
      const segments = currentPath.split("/");
      let accumulated = "";
      for (const seg of segments) {
        accumulated = accumulated ? `${accumulated}/${seg}` : seg;
        const ancestor = findNode(data.nodes, accumulated);
        if (ancestor) ancestors.push(ancestor);
      }
      ancestors.reverse();
    }
    let yOffset = 0;
    for (const node of nodes) {
      const isDimmed = expandedNode !== null && expandedNode !== node.path;
      const isExpanded = expandedNode === node.path;
      const row = renderNodeRow(node, scaleState, layout, yOffset, { isDimmed, isExpanded, parentNode: ancestors[0] || null });
      renderedRows.push(row);
      yOffset += row.height;
      if (isExpanded && node.issues.length > 0) {
        const issueRows = renderIssueRows(node, scaleState, layout, yOffset, expandedShowAll, ancestors);
        renderedRows.push(...issueRows);
        yOffset += issueRows.reduce((sum, r) => sum + r.height, 0);
      }
    }
    syncSvgHeight(layout, yOffset);
    const totalHeight = yOffset + getAxisHeight();
    renderAxis(scaleState, layout, totalHeight);
    renderGridLines(scaleState, layout, totalHeight);
    renderTodayLine(scaleState, layout, totalHeight);
    const okrs = collectOkrs(data.nodes);
    renderMilestoneLines(scaleState, layout, totalHeight, okrs);
    if (currentPath !== "") {
      const parentNode = findNode(data.nodes, currentPath);
      if (parentNode) {
        const checkpoints = allCheckpoints(parentNode);
        const filtered = checkpoints.filter((m) => !okrs.some((o) => o.name === m.name && o.date === m.date));
        renderMilestoneLines(scaleState, layout, totalHeight, filtered);
      }
    }
  }
  function onZoom() {
    renderChart();
  }
  function navigateTo(path) {
    currentPath = path;
    selectedNode = null;
    expandedNode = null;
    expandedShowAll = false;
    closePanel();
    updateBreadcrumb();
    resetZoom(scaleState, layout);
    renderChart();
  }
  window.__nav = navigateTo;
  function handleRowClick(row) {
    if (row.type === "node" && row.node) {
      const node = row.node;
      if (node.children.length === 0 && node.issues.length > 0) {
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
  function handleRowDblClick(row) {
    if (row.type === "node" && row.node && row.node.children.length > 0) {
      navigateTo(row.node.path);
    }
  }
  function setRange(global) {
    useGlobalRange = global;
    document.getElementById("btn-fitted")?.classList.toggle("active", !global);
    document.getElementById("btn-global")?.classList.toggle("active", global);
    resetZoom(scaleState, layout);
    renderChart();
  }
  window.__setRange = setRange;
  var tooltipEl = document.getElementById("tooltip");
  function showTooltip(e, html) {
    tooltipEl.innerHTML = html;
    tooltipEl.style.display = "block";
    tooltipEl.style.left = `${e.clientX + 12}px`;
    tooltipEl.style.top = `${e.clientY + 12}px`;
  }
  function hideTooltip() {
    tooltipEl.style.display = "none";
  }
  window.__chartState = {
    get currentPath() {
      return currentPath;
    },
    get expandedNode() {
      return expandedNode;
    },
    get visibleNodes() {
      return getVisibleNodes();
    },
    get renderedRows() {
      return renderedRows;
    }
  };
  layout = getLayoutElements();
  var initialRange = computeTimeRange(getVisibleNodes());
  var initialWidth = getTimelineWidth();
  scaleState = createScale(initialRange, initialWidth);
  setupZoom(scaleState, layout, onZoom);
  layout.labelsEl.addEventListener("click", (e) => {
    const target = e.target.closest(".chart-row");
    if (!target) return;
    const idx = Array.from(layout.labelsEl.children).indexOf(target);
    if (idx >= 0 && renderedRows[idx]) {
      handleRowClick(renderedRows[idx]);
    }
  });
  layout.labelsEl.addEventListener("dblclick", (e) => {
    const target = e.target.closest(".chart-row");
    if (!target) return;
    const idx = Array.from(layout.labelsEl.children).indexOf(target);
    if (idx >= 0 && renderedRows[idx]) {
      handleRowDblClick(renderedRows[idx]);
    }
  });
  layout.timelineSvg.addEventListener("click", (e) => {
    const target = e.target;
    const path = target.dataset?.path;
    if (path) {
      const row = renderedRows.find((r) => r.type === "node" && r.node?.path === path);
      if (row) handleRowClick(row);
    }
  });
  layout.timelineSvg.addEventListener("dblclick", (e) => {
    const target = e.target;
    const path = target.dataset?.path;
    if (path) {
      const row = renderedRows.find((r) => r.type === "node" && r.node?.path === path);
      if (row) handleRowDblClick(row);
    }
  });
  layout.labelsEl.addEventListener("mouseover", (e) => {
    const target = e.target.closest(".chart-row");
    if (!target) return;
    const idx = Array.from(layout.labelsEl.children).indexOf(target);
    const row = renderedRows[idx];
    if (!row) return;
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
      const dates = n.has_timeline ? `${n.start} \u2192 ${n.end}` : n.eff_start ? `~${n.eff_start} \u2192 ~${n.eff_end}` : "No timeline";
      showTooltip(e, `<b>${escapeHtml(n.name)}</b><br/>${dates}<br/>Status: ${n.status}`);
    }
    target.classList.add("highlighted");
    const issueRef = target.dataset.issueRef;
    const nodePath = target.dataset.path;
    if (issueRef) {
      layout.barsGroup.querySelectorAll(`.issue-bar[data-issue-ref="${CSS.escape(issueRef)}"]`).forEach((el) => el.classList.add("highlighted"));
    } else if (nodePath) {
      layout.barsGroup.querySelectorAll(`.node-bar[data-path="${CSS.escape(nodePath)}"]`).forEach((el) => el.classList.add("highlighted"));
    }
  });
  layout.labelsEl.addEventListener("mouseout", (e) => {
    const target = e.target.closest(".chart-row");
    if (!target) return;
    hideTooltip();
    target.classList.remove("highlighted");
    const issueRef = target.dataset.issueRef;
    const nodePath = target.dataset.path;
    if (issueRef) {
      layout.barsGroup.querySelectorAll(`.issue-bar[data-issue-ref="${CSS.escape(issueRef)}"]`).forEach((el) => el.classList.remove("highlighted"));
    } else if (nodePath) {
      layout.barsGroup.querySelectorAll(`.node-bar[data-path="${CSS.escape(nodePath)}"]`).forEach((el) => el.classList.remove("highlighted"));
    }
  });
  window.addEventListener("resize", () => {
    updateScaleRange(scaleState, getTimelineWidth());
    renderChart();
  });
  updateBreadcrumb();
  renderChart();
})();
