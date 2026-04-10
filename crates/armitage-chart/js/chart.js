"use strict";
(() => {
  // ts/chart.ts
  var data = window.__CHART_DATA__;
  var currentPath = "";
  var useGlobalRange = false;
  var selectedNode = null;
  var expandedNode = null;
  var expandedShowAll = false;
  var visibleNodes = [];
  var seriesEntries = [];
  var chartEl = document.getElementById("chart");
  var breadcrumbEl = document.getElementById("breadcrumb");
  var toggleBtn = document.getElementById("toggle-range");
  var panelEl = document.getElementById("panel");
  var panelContentEl = document.getElementById("panel-content");
  var chart = echarts.init(chartEl);
  var STATUS_COLORS = {
    active: "#3b82f6",
    completed: "#6b7280",
    paused: "#f59e0b",
    cancelled: "#ef4444"
  };
  var NO_TIMELINE_COLOR = "rgba(107, 114, 128, 0.15)";
  var NO_TIMELINE_BORDER = "rgba(107, 114, 128, 0.4)";
  function parseDate(s) {
    return (/* @__PURE__ */ new Date(s + "T00:00:00")).getTime();
  }
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
  function computeTimeRange(nodes) {
    if (useGlobalRange && data.global_start && data.global_end) {
      return [parseDate(data.global_start), parseDate(data.global_end)];
    }
    let min = Infinity;
    let max = -Infinity;
    for (const n of nodes) {
      const s = n.eff_start;
      const e = n.eff_end;
      if (s) min = Math.min(min, parseDate(s));
      if (e) max = Math.max(max, parseDate(e));
    }
    if (min === Infinity || max === -Infinity) {
      const now = /* @__PURE__ */ new Date();
      min = new Date(now.getFullYear(), 0, 1).getTime();
      max = new Date(now.getFullYear(), 11, 31).getTime();
    }
    const pad = 30 * 24 * 3600 * 1e3;
    return [min - pad, max + pad];
  }
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
  function clusterTicks(issues, parentStart, parentRange, threshold) {
    const dated = issues.filter((i) => i.target_date);
    if (dated.length === 0) return [];
    const sorted = [...dated].sort(
      (a, b) => a.target_date.localeCompare(b.target_date)
    );
    const clusters = [];
    let curCluster = {
      relXs: [],
      overdueCount: 0
    };
    for (const issue of sorted) {
      const relX = (parseDate(issue.target_date) - parentStart) / parentRange;
      if (curCluster.relXs.length > 0 && relX - curCluster.relXs[curCluster.relXs.length - 1] > threshold) {
        const avg = curCluster.relXs.reduce((a, b) => a + b, 0) / curCluster.relXs.length;
        clusters.push({
          relX: avg,
          count: curCluster.relXs.length,
          overdue: curCluster.overdueCount > 0
        });
        curCluster = { relXs: [], overdueCount: 0 };
      }
      curCluster.relXs.push(relX);
      if (relX > 1) curCluster.overdueCount++;
    }
    if (curCluster.relXs.length > 0) {
      const avg = curCluster.relXs.reduce((a, b) => a + b, 0) / curCluster.relXs.length;
      clusters.push({
        relX: avg,
        count: curCluster.relXs.length,
        overdue: curCluster.overdueCount > 0
      });
    }
    return clusters;
  }
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
  function escapeHtml(s) {
    const div = document.createElement("div");
    div.textContent = s;
    return div.innerHTML;
  }
  function issueUrl(ref) {
    const match = ref.match(/^(.+?)\/(.+?)#(\d+)$/);
    if (!match) return "#";
    return `https://github.com/${match[1]}/${match[2]}/issues/${match[3]}`;
  }
  function showPanel(node) {
    selectedNode = node;
    let html = "";
    html += `<h2>${escapeHtml(node.name)}</h2>`;
    html += `<span class="panel-status ${node.status}">${node.status}</span>`;
    if (node.description) {
      html += `<div class="panel-section">`;
      html += `<h3>Description</h3>`;
      html += `<div class="panel-desc">${escapeHtml(node.description)}</div>`;
      html += `</div>`;
    }
    html += `<div class="panel-section">`;
    html += `<h3>Timeline</h3>`;
    html += `<div class="panel-meta">`;
    if (node.has_timeline) {
      html += `<span class="label">Start:</span> ${node.start}<br/>`;
      html += `<span class="label">End:</span> ${node.end}`;
    } else if (node.eff_start) {
      html += `<span class="label">Derived:</span> ${node.eff_start} &rarr; ${node.eff_end}`;
    } else {
      html += `<span class="label">No timeline</span>`;
    }
    html += `</div></div>`;
    if (node.owners.length > 0 || node.team) {
      html += `<div class="panel-section">`;
      html += `<h3>People</h3>`;
      html += `<div class="panel-meta">`;
      if (node.owners.length > 0) {
        html += `<span class="label">Owners:</span> ${node.owners.map(escapeHtml).join(", ")}<br/>`;
      }
      if (node.team) {
        html += `<span class="label">Team:</span> ${escapeHtml(node.team)}`;
      }
      html += `</div></div>`;
    }
    const checkpoints = allCheckpoints(node);
    if (checkpoints.length > 0) {
      html += `<div class="panel-section">`;
      html += `<h3>Milestones</h3>`;
      html += `<ul class="panel-milestones">`;
      for (const m of checkpoints) {
        html += `<li>&diams; ${escapeHtml(m.name)} <span class="ms-date">${m.date}</span>`;
        if (m.description) html += `<br/><span class="ms-date">${escapeHtml(m.description)}</span>`;
        html += `</li>`;
      }
      html += `</ul></div>`;
    }
    if (node.issues.length > 0) {
      html += `<div class="panel-section">`;
      html += `<h3>Issues (${node.issues.length})</h3>`;
      html += `<ul class="panel-issues">`;
      for (const issue of node.issues) {
        const url = issueUrl(issue.issue_ref);
        const label = issue.title ? `${escapeHtml(issue.title)} <span class="issue-ref">${escapeHtml(issue.issue_ref)}</span>` : escapeHtml(issue.issue_ref);
        html += `<li><a class="panel-issue-link" href="${url}" target="_blank" rel="noopener">${label}</a></li>`;
      }
      html += `</ul></div>`;
    }
    if (node.children.length > 0) {
      html += `<div class="panel-section">`;
      html += `<h3>Children (${node.children.length})</h3>`;
      html += `<ul class="panel-children">`;
      for (const c of node.children) {
        const color = STATUS_COLORS[c.status] || STATUS_COLORS.active;
        const dates = c.has_timeline ? `${c.start} &rarr; ${c.end}` : c.eff_start ? `~${c.eff_start}` : "";
        html += `<li>`;
        html += `<span class="dot" style="background:${color}"></span>`;
        html += `<span class="child-name">${escapeHtml(c.name)}</span>`;
        if (dates) html += `<span class="child-dates">${dates}</span>`;
        html += `</li>`;
      }
      html += `</ul>`;
      html += `<button class="btn-drill" onclick="window.__nav('${node.path}')">Drill into ${escapeHtml(node.name)} &rsaquo;</button>`;
      html += `</div>`;
    }
    panelContentEl.innerHTML = html;
    panelEl.classList.add("open");
    chart.resize();
  }
  function closePanel() {
    selectedNode = null;
    panelEl.classList.remove("open");
    chart.resize();
  }
  window.__closePanel = closePanel;
  function buildOption() {
    const nodes = getVisibleNodes();
    visibleNodes = nodes;
    const [xMin, xMax] = computeTimeRange(nodes);
    const categories = [];
    const seriesData = [];
    const entries = [];
    const INITIAL_ISSUE_LIMIT = 7;
    for (const node of [...nodes].reverse()) {
      const catIdx = categories.length;
      categories.push(node.name);
      seriesData.push({
        value: [
          node.eff_start ? parseDate(node.eff_start) : xMin,
          node.eff_end ? parseDate(node.eff_end) : xMax,
          catIdx
        ]
      });
      entries.push({ type: "node", node });
      if (expandedNode === node.path && node.issues.length > 0) {
        const sorted = sortIssues(node.issues, node.end);
        const allSorted = [...sorted.overdue, ...sorted.onTrack, ...sorted.noDates];
        const limit = expandedShowAll ? allSorted.length : INITIAL_ISSUE_LIMIT;
        const visible = allSorted.slice(0, limit);
        let insertedOverdue = false;
        let insertedSeparator = false;
        for (let i = 0; i < visible.length; i++) {
          const issue = visible[i];
          const isOverdue = sorted.overdue.includes(issue);
          const isOnTrackOrNoDates = !isOverdue;
          if (isOnTrackOrNoDates && !insertedSeparator && insertedOverdue) {
            const sepIdx = categories.length;
            categories.push("\u2500\u2500\u2500");
            seriesData.push({ value: [xMin, xMin, sepIdx] });
            entries.push({ type: "separator" });
            insertedSeparator = true;
          }
          if (isOverdue) insertedOverdue = true;
          const catLabel = issue.title ? (isOverdue ? "\u26A0 " : "") + issue.title : issue.issue_ref;
          const issueIdx = categories.length;
          categories.push(catLabel);
          const iStart = issue.start_date ? parseDate(issue.start_date) : xMin;
          const iEnd = issue.target_date ? parseDate(issue.target_date) : xMax;
          seriesData.push({ value: [iStart, iEnd, issueIdx] });
          entries.push({ type: "issue", issue, parentNode: node });
        }
        if (!expandedShowAll && allSorted.length > INITIAL_ISSUE_LIMIT) {
          const remaining = allSorted.length - INITIAL_ISSUE_LIMIT;
          const moreIdx = categories.length;
          categories.push(`\u25BE Show all ${allSorted.length} issues (${remaining} more)`);
          seriesData.push({ value: [xMin, xMin, moreIdx] });
          entries.push({ type: "show-more", parentNode: node, remaining });
        }
      }
    }
    seriesEntries = entries;
    const okrs = collectOkrs(data.nodes);
    const okrLines = okrs.map((m) => ({
      xAxis: parseDate(m.date),
      name: m.name,
      _okr: m
    }));
    const parentCheckpointLines = [];
    if (currentPath !== "") {
      const parentNode = findNode(data.nodes, currentPath);
      if (parentNode) {
        const seen = new Set(okrs.map((m) => `${m.name}|${m.date}`));
        for (const m of allCheckpoints(parentNode)) {
          const key = `${m.name}|${m.date}`;
          if (!seen.has(key)) {
            seen.add(key);
            parentCheckpointLines.push({
              xAxis: parseDate(m.date),
              name: m.name,
              _okr: m
              // reuse same shape for tooltip
            });
          }
        }
      }
    }
    const todayLine = {
      xAxis: (/* @__PURE__ */ new Date()).setHours(0, 0, 0, 0),
      name: "Today",
      _okr: null
    };
    const allVerticalLines = [todayLine, ...okrLines, ...parentCheckpointLines];
    return {
      tooltip: {
        trigger: "item",
        formatter: (params) => {
          const idx = params.dataIndex;
          const n = visibleNodes[idx];
          if (!n) return "";
          const dates = n.has_timeline ? `${n.start} &rarr; ${n.end}` : n.eff_start ? `~${n.eff_start} &rarr; ~${n.eff_end} (derived)` : "No fixed timeline";
          const parts = [`<b>${n.name}</b>`, dates, `Status: ${n.status}`];
          if (n.owners.length > 0) parts.push(`Owners: ${n.owners.join(", ")}`);
          if (n.team) parts.push(`Team: ${n.team}`);
          const ms = allCheckpoints(n);
          if (ms.length > 0) {
            parts.push("");
            parts.push(`<b>Milestones:</b>`);
            for (const m of ms) {
              parts.push(`&diams; ${m.name} (${m.date})`);
            }
          }
          parts.push("", "<i>Click for details</i>");
          return parts.join("<br/>");
        }
      },
      grid: {
        top: 40,
        bottom: 40,
        left: 20,
        right: 20,
        containLabel: true
      },
      xAxis: {
        type: "time",
        min: xMin,
        max: xMax,
        axisLabel: { color: "#8b949e" },
        axisLine: { lineStyle: { color: "#30363d" } },
        splitLine: {
          show: true,
          lineStyle: { color: "#21262d", type: "dashed" }
        }
      },
      yAxis: {
        type: "category",
        data: categories,
        axisLabel: {
          formatter: (value) => value,
          rich: {},
          color: (value, index) => {
            const entry = seriesEntries[index];
            if (!entry) return "#e6edf3";
            if (entry.type === "separator") return "#21262d";
            if (entry.type === "show-more") return "#484f58";
            if (entry.type === "issue") {
              const isOverdue = entry.issue.target_date && entry.parentNode.end && entry.issue.target_date > entry.parentNode.end;
              return isOverdue ? "#f85149" : "#8b949e";
            }
            return "#e6edf3";
          },
          fontWeight: (value, index) => {
            const entry = seriesEntries[index];
            return entry?.type === "node" ? "bold" : "normal";
          },
          fontSize: (value, index) => {
            const entry = seriesEntries[index];
            if (entry?.type === "issue") return 11;
            if (entry?.type === "separator") return 9;
            if (entry?.type === "show-more") return 11;
            return 13;
          }
        },
        axisLine: { show: false },
        axisTick: { show: false }
      },
      series: [
        {
          type: "custom",
          renderItem: renderBar,
          encode: { x: [0, 1], y: 2 },
          data: seriesData
        },
        // Invisible line series that carries markLine for vertical lines
        // (today, OKRs, checkpoints). markLine on custom series is unreliable.
        {
          type: "line",
          data: [],
          symbol: "none",
          silent: true,
          markLine: {
            silent: false,
            symbol: ["none", "none"],
            label: {
              show: true,
              position: "start",
              formatter: (p) => p.name,
              fontSize: 10,
              color: "#8b949e"
            },
            lineStyle: {
              type: "dashed",
              width: 1
            },
            data: allVerticalLines.map((line) => {
              const isToday = line === todayLine;
              const isOkr = okrLines.includes(line);
              if (isToday) {
                return {
                  ...line,
                  lineStyle: {
                    color: "rgba(239, 68, 68, 0.7)",
                    type: "solid",
                    width: 2
                  },
                  label: {
                    color: "#ef4444"
                  }
                };
              }
              return {
                ...line,
                lineStyle: {
                  color: isOkr ? "rgba(167, 139, 250, 0.5)" : "rgba(245, 158, 11, 0.5)"
                },
                label: {
                  color: isOkr ? "#a78bfa" : "#f59e0b"
                }
              };
            }),
            tooltip: {
              formatter: (p) => {
                if (p.name === "Today") return "Today";
                const m = p.data?._okr;
                if (!m) return p.name;
                return [
                  `<b>${m.name}</b>`,
                  m.date,
                  m.description || ""
                ].filter(Boolean).join("<br/>");
              }
            }
          }
        }
      ],
      backgroundColor: "transparent"
    };
  }
  function renderBar(params, api) {
    const node = visibleNodes[params.dataIndex];
    if (!node) return { type: "group", children: [] };
    const yIdx = api.value(2);
    const start = api.coord([api.value(0), yIdx]);
    const end = api.coord([api.value(1), yIdx]);
    const bandWidth = api.size([0, 1])[1];
    const x = start[0];
    const y = start[1] - bandWidth * 0.4;
    const width = end[0] - start[0];
    const height = bandWidth * 0.8;
    if (width <= 0) return { type: "group", children: [] };
    const children = [];
    const isSelected = selectedNode?.path === node.path;
    const statusColor = STATUS_COLORS[node.status] || STATUS_COLORS.active;
    const hasTimeline = node.has_timeline;
    children.push({
      type: "rect",
      shape: { x, y, width, height, r: 4 },
      style: {
        fill: hasTimeline ? `${statusColor}22` : NO_TIMELINE_COLOR,
        stroke: isSelected ? "#e6edf3" : hasTimeline ? `${statusColor}55` : NO_TIMELINE_BORDER,
        lineWidth: isSelected ? 2 : 1,
        lineDash: hasTimeline || isSelected ? null : [4, 3]
      }
    });
    if (node.children.length > 0) {
      const childrenWithTimeline = node.children.filter(
        (c) => c.eff_start && c.eff_end
      );
      if (childrenWithTimeline.length > 0) {
        const outerStart = api.value(0);
        const outerEnd = api.value(1);
        const outerRange = outerEnd - outerStart;
        const childStarts = childrenWithTimeline.map(
          (c) => parseDate(c.eff_start)
        );
        const childEnds = childrenWithTimeline.map(
          (c) => parseDate(c.eff_end)
        );
        const spanStart = Math.min(...childStarts);
        const spanEnd = Math.max(...childEnds);
        const relStart = Math.max(0, (spanStart - outerStart) / outerRange);
        const relEnd = Math.min(1, (spanEnd - outerStart) / outerRange);
        const fillX = x + relStart * width;
        const fillW = (relEnd - relStart) * width;
        children.push({
          type: "rect",
          shape: { x: fillX, y: y + 1, width: fillW, height: height - 2, r: 4 },
          style: {
            fill: new echarts.graphic.LinearGradient(0, 0, 1, 0, [
              { offset: 0, color: "rgba(88, 166, 255, 0.35)" },
              { offset: 1, color: "rgba(88, 166, 255, 0.15)" }
            ])
          }
        });
      }
      const badgeText = `\xD7${node.children.length} children`;
      const badgeX = x + width - 20;
      const badgeY = y + height / 2;
      children.push({
        type: "text",
        style: {
          text: badgeText,
          x: badgeX,
          y: badgeY,
          fill: "#58a6ff",
          fontSize: 11,
          fontWeight: 600,
          textAlign: "right",
          textVerticalAlign: "middle",
          backgroundColor: "rgba(13, 17, 23, 0.7)",
          borderRadius: 3,
          padding: [2, 6]
        }
      });
    }
    if (node.children.length === 0 && node.issues.length > 0) {
      const outerStart = api.value(0);
      const outerEnd = api.value(1);
      const outerRange = outerEnd - outerStart;
      const clusters = clusterTicks(node.issues, outerStart, outerRange, 0.02);
      for (const cluster of clusters) {
        const clampedX = Math.max(0, Math.min(1, cluster.relX));
        const tickX = x + clampedX * width;
        const tickW = cluster.count > 1 ? 6 : 3;
        const tickH = 14;
        const tickY = y + (height - tickH) / 2;
        const tickColor = cluster.overdue ? "#f85149" : "#58a6ff";
        const tickOpacity = cluster.overdue ? 0.9 : 0.7;
        children.push({
          type: "rect",
          shape: {
            x: tickX - tickW / 2,
            y: tickY,
            width: tickW,
            height: tickH,
            r: 1
          },
          style: {
            fill: tickColor,
            opacity: tickOpacity
          }
        });
        if (cluster.count > 1) {
          children.push({
            type: "text",
            style: {
              text: `${cluster.count}`,
              x: tickX,
              y: tickY - 2,
              fill: tickColor,
              fontSize: 8,
              textAlign: "center",
              textVerticalAlign: "bottom",
              opacity: 0.8
            }
          });
        }
      }
      const overdueCount = node.issues.filter(
        (i) => i.target_date && node.end && i.target_date > node.end
      ).length;
      const badgeX = x + width - 8;
      const badgeY = y + height / 2;
      if (overdueCount > 0) {
        children.push({
          type: "text",
          style: {
            text: `${node.issues.length} issues \xB7 {overdue|${overdueCount} overdue}`,
            x: badgeX,
            y: badgeY,
            fill: "#8b949e",
            fontSize: 11,
            fontWeight: 600,
            textAlign: "right",
            textVerticalAlign: "middle",
            backgroundColor: "rgba(13, 17, 23, 0.7)",
            borderRadius: 3,
            padding: [2, 6],
            rich: {
              overdue: {
                fill: "#f85149",
                fontSize: 11,
                fontWeight: 600
              }
            }
          }
        });
      } else {
        children.push({
          type: "text",
          style: {
            text: `${node.issues.length} issues`,
            x: badgeX,
            y: badgeY,
            fill: "#8b949e",
            fontSize: 11,
            fontWeight: 600,
            textAlign: "right",
            textVerticalAlign: "middle",
            backgroundColor: "rgba(13, 17, 23, 0.7)",
            borderRadius: 3,
            padding: [2, 6]
          }
        });
      }
    }
    const nodeMilestones = currentPath === "" ? allCheckpoints(node) : [];
    if (nodeMilestones.length > 0) {
      for (const m of nodeMilestones) {
        const mDate = parseDate(m.date);
        const mx = api.coord([mDate, api.value(2)])[0];
        const mColor = "#f59e0b";
        children.push({
          type: "line",
          shape: { x1: mx, y1: y, x2: mx, y2: y + height },
          style: {
            stroke: mColor,
            lineWidth: 1,
            lineDash: [3, 2],
            opacity: 0.7
          }
        });
        const ds = 5;
        const dy = y + 2 + ds;
        children.push({
          type: "path",
          shape: {
            d: `M${mx},${dy - ds}L${mx + ds},${dy}L${mx},${dy + ds}L${mx - ds},${dy}Z`
          },
          style: {
            fill: mColor,
            opacity: 0.9
          }
        });
        children.push({
          type: "text",
          style: {
            text: m.name,
            x: mx,
            y: y - 2,
            fill: mColor,
            fontSize: 9,
            textAlign: "center",
            textVerticalAlign: "bottom",
            opacity: 0.8
          }
        });
      }
    }
    if (node.children.length > 0) {
      const arrowX = x + width - 12;
      const arrowY = y + height / 2;
      children.push({
        type: "path",
        shape: {
          d: `M${arrowX},${arrowY - 4}L${arrowX + 6},${arrowY}L${arrowX},${arrowY + 4}Z`
        },
        style: {
          fill: hasTimeline ? statusColor : "#6b7280",
          opacity: 0.6
        }
      });
    }
    return { type: "group", children };
  }
  function navigateTo(path) {
    currentPath = path;
    selectedNode = null;
    expandedNode = null;
    expandedShowAll = false;
    closePanel();
    updateBreadcrumb();
    renderChart();
  }
  window.__nav = navigateTo;
  chart.on("click", (params) => {
    const idx = params.dataIndex;
    const entry = seriesEntries[idx];
    if (entry && entry.type === "issue") {
      const url = issueUrl(entry.issue.issue_ref);
      if (url !== "#") window.open(url, "_blank", "noopener");
      return;
    }
    if (entry && entry.type === "show-more") {
      expandedShowAll = true;
      renderChart();
      return;
    }
    const node = entry?.type === "node" ? entry.node : void 0;
    if (!node) return;
    if (node.children.length === 0 && node.issues.length > 0) {
      if (expandedNode === node.path) {
        expandedNode = null;
        expandedShowAll = false;
      } else {
        expandedNode = node.path;
        expandedShowAll = false;
      }
      selectedNode = null;
      closePanel();
      renderChart();
    } else {
      showPanel(node);
      renderChart();
    }
  });
  chart.on("dblclick", (params) => {
    const node = visibleNodes[params.dataIndex];
    if (node && node.children.length > 0) {
      navigateTo(node.path);
    }
  });
  function renderChart() {
    chart.setOption(buildOption(), true);
  }
  toggleBtn.addEventListener("click", () => {
    useGlobalRange = !useGlobalRange;
    toggleBtn.textContent = useGlobalRange ? "Show Fitted Range" : "Show Global Range";
    renderChart();
  });
  window.addEventListener("resize", () => chart.resize());
  updateBreadcrumb();
  renderChart();
})();
