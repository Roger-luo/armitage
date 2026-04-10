"use strict";
(() => {
  // crates/armitage-chart/ts/chart.ts
  var data = window.__CHART_DATA__;
  var currentPath = "";
  var useGlobalRange = false;
  var selectedNode = null;
  var visibleNodes = [];
  var chartEl = document.getElementById("chart");
  var breadcrumbEl = document.getElementById("breadcrumb");
  var btnFitted = document.getElementById("btn-fitted");
  var btnGlobal = document.getElementById("btn-global");
  var panelEl = document.getElementById("panel");
  var panelContentEl = document.getElementById("panel-content");
  var chart = echarts.init(chartEl);
  var STATUS_COLORS = {
    active: "#3b82f6",
    completed: "#6b7280",
    paused: "#f59e0b",
    cancelled: "#ef4444"
  };
  function cssVar(name) {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  }
  function getThemeColors() {
    return {
      text: cssVar("--text"),
      textMuted: cssVar("--text-muted"),
      axisLine: cssVar("--chart-axis-line"),
      grid: cssVar("--chart-grid"),
      axis: cssVar("--chart-axis"),
      noTlFill: cssVar("--no-tl-fill"),
      noTlBorder: cssVar("--no-tl-border"),
    };
  }
  var tc = getThemeColors();
  window.__onThemeChange = function() {
    tc = getThemeColors();
    renderChart();
  };
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
  function allIssues(node) {
    const result = [...node.issues];
    for (const c of node.children) {
      result.push(...allIssues(c));
    }
    return result;
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
    if (node.overflow_end) {
      html += `<div class="panel-section panel-overflow">`;
      html += `<h3>⚠ Timeline Overflow</h3>`;
      html += `<div class="panel-meta">Issues target as late as <b>${node.overflow_end}</b>`;
      if (node.end) html += `, but node ends <b>${node.end}</b>`;
      html += `</div></div>`;
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
    const issues = allIssues(node);
    if (issues.length > 0) {
      html += `<div class="panel-section">`;
      html += `<h3>Issues (${issues.length})</h3>`;
      if (node.issue_start || node.issue_end) {
        html += `<div class="panel-meta" style="margin-bottom:8px">`;
        if (node.issue_start) html += `<span class="label">Earliest start:</span> ${node.issue_start}<br/>`;
        if (node.issue_end) html += `<span class="label">Latest target:</span> ${node.issue_end}`;
        html += `</div>`;
      }
      html += `<ul class="panel-issues">`;
      for (const issue of issues) {
        const url = issueUrl(issue.issue_ref);
        let label = issue.title ? escapeHtml(issue.title) : escapeHtml(issue.issue_ref);
        let meta = `<span class="issue-ref">${escapeHtml(issue.issue_ref)}</span>`;
        if (issue.target_date) {
          const isOverflow = node.end && issue.target_date > node.end;
          meta += isOverflow
            ? ` <span class="issue-overflow">&rarr; ${issue.target_date}</span>`
            : ` <span class="issue-date">&rarr; ${issue.target_date}</span>`;
        }
        html += `<li><a class="panel-issue-link" href="${url}" target="_blank" rel="noopener">${label}</a><br/>${meta}</li>`;
      }
      html += `</ul></div>`;
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
    const categories = nodes.map((n) => n.name).reverse();
    const seriesData = nodes.map((n, i) => ({
      value: [
        n.eff_start ? parseDate(n.eff_start) : xMin,
        n.eff_end ? parseDate(n.eff_end) : xMax,
        categories.length - 1 - i
      ]
    }));
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
        axisLabel: { color: tc.axis },
        axisLine: { lineStyle: { color: tc.axisLine } },
        splitLine: {
          show: true,
          lineStyle: { color: tc.grid, type: "dashed" }
        }
      },
      yAxis: {
        type: "category",
        data: categories,
        axisLabel: {
          color: tc.text,
          fontWeight: "bold",
          fontSize: 13
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
              color: tc.textMuted
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
        fill: hasTimeline ? `${statusColor}22` : tc.noTlFill,
        stroke: isSelected ? tc.text : hasTimeline ? `${statusColor}55` : tc.noTlBorder,
        lineWidth: isSelected ? 2 : 1,
        lineDash: hasTimeline || isSelected ? null : [4, 3]
      }
    });
    // Build sub-bars: child nodes + all descendant issues with dates
    const subBars = [];
    for (const c of node.children) {
      if (c.eff_start && c.eff_end) {
        subBars.push({ type: "node", start: c.eff_start, end: c.eff_end, color: STATUS_COLORS[c.status] || STATUS_COLORS.active, overflowStart: c.overflow_start, overflowEnd: c.overflow_end, label: c.name });
      }
    }
    // Use overflow_start as the boundary — it's the earliest violated deadline
    // from any descendant. Falls back to the node's own end / eff_end.
    const issueDeadline = node.overflow_start || node.end || node.eff_end;
    for (const issue of allIssues(node)) {
      if (issue.start_date || issue.target_date) {
        const iStart = issue.start_date || issue.target_date;
        const iEnd = issue.target_date || issue.start_date;
        const overflows = issueDeadline && iEnd > issueDeadline;
        subBars.push({ type: "issue", start: iStart, end: iEnd, overflows, label: issue.title || issue.issue_ref, issueRef: issue.issue_ref });
      }
    }
    if (subBars.length > 0) {
      const barAreaTop = y + 4;
      const barAreaHeight = height - 8;
      const maxBars = Math.min(subBars.length, 8);
      const barH = Math.max(
        3,
        Math.min(10, (barAreaHeight - (maxBars - 1) * 2) / maxBars)
      );
      const gap = Math.max(1, (barAreaHeight - maxBars * barH) / (maxBars + 1));
      const outerStart = api.value(0);
      const outerEnd = api.value(1);
      const outerRange = outerEnd - outerStart;
      for (let i = 0; i < maxBars; i++) {
        const sub = subBars[i];
        const cStart = parseDate(sub.start);
        const cEnd = parseDate(sub.end);
        const relStart = Math.max(0, (cStart - outerStart) / outerRange);
        // Don't cap issue bars at parent edge — let them extend into overflow
        const relEnd = sub.type === "issue"
          ? (cEnd - outerStart) / outerRange
          : Math.min(1, (cEnd - outerStart) / outerRange);
        const cx = x + relStart * width;
        const cw = Math.max((relEnd - relStart) * width, 2);
        const cy = barAreaTop + gap + i * (barH + gap);
        const opacity = 0.6 + 0.3 * (1 - i / maxBars);
        if (sub.type === "issue") {
          if (sub.overflows && issueDeadline) {
            // Split bar: green up to deadline, purple in overflow
            const splitDate = parseDate(issueDeadline);
            const relSplit = Math.max(relStart, Math.min(relEnd, (splitDate - outerStart) / outerRange));
            const splitX = x + relSplit * width;
            const greenW = Math.max(splitX - cx, 0);
            const purpleW = Math.max(cx + cw - splitX, 0);
            if (greenW > 0) {
              children.push({
                type: "rect",
                shape: { x: cx, y: cy, width: greenW, height: barH, r: [barH / 2, 0, 0, barH / 2] },
                style: { fill: "#22c55e44", stroke: "#22c55e88", lineWidth: 1, lineDash: [2, 2], opacity }
              });
            }
            if (purpleW > 0) {
              children.push({
                type: "rect",
                shape: { x: splitX, y: cy, width: purpleW, height: barH, r: [0, barH / 2, barH / 2, 0] },
                style: { fill: "#8b5cf644", stroke: "#8b5cf688", lineWidth: 1, lineDash: [2, 2], opacity }
              });
            }
          } else {
            // Fully on-track: green
            children.push({
              type: "rect",
              shape: { x: cx, y: cy, width: cw, height: barH, r: barH / 2 },
              style: { fill: "#22c55e44", stroke: "#22c55e88", lineWidth: 1, lineDash: [2, 2], opacity }
            });
          }
        } else {
          // Child node bar
          children.push({
            type: "rect",
            shape: { x: cx, y: cy, width: cw, height: barH, r: 2 },
            style: { fill: sub.color, opacity }
          });
          // Overflow extension for child nodes
          if (sub.overflowEnd && sub.overflowStart) {
            const oStart = parseDate(sub.overflowStart);
            const oEnd = parseDate(sub.overflowEnd);
            const relOStart = (oStart - outerStart) / outerRange;
            const relOEnd = (oEnd - outerStart) / outerRange;
            const ox = x + relOStart * width;
            const ow = (relOEnd - relOStart) * width;
            if (ow > 0) {
              children.push({
                type: "rect",
                shape: { x: ox, y: cy, width: ow, height: barH, r: [0, 2, 2, 0] },
                style: {
                  fill: "rgba(239, 68, 68, 0.3)",
                  stroke: "rgba(239, 68, 68, 0.6)",
                  lineWidth: 1,
                  lineDash: [3, 2]
                }
              });
            }
          }
        }
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
    // Only show outer bar red zone when overflow exceeds the bar's own timeline.
    // If the overflow is contained within the node's timeline (e.g. a child
    // milestone overflows but the product line doesn't), sub-bars handle it.
    const barEnd = node.end || node.eff_end;
    if (node.overflow_end && node.overflow_start && barEnd && node.overflow_end > barEnd) {
      const overflowX = api.coord([parseDate(node.overflow_start), yIdx])[0];
      const overflowEnd = api.coord([parseDate(node.overflow_end), yIdx]);
      const overflowWidth = overflowEnd[0] - overflowX;
      if (overflowWidth > 0) {
        children.push({
          type: "rect",
          shape: { x: overflowX, y: y + 2, width: overflowWidth, height: height - 4, r: [0, 4, 4, 0] },
          style: {
            fill: "rgba(239, 68, 68, 0.15)",
            stroke: "rgba(239, 68, 68, 0.5)",
            lineWidth: 1,
            lineDash: [4, 2]
          }
        });
        children.push({
          type: "text",
          style: {
            text: "\u26a0",
            x: overflowX + overflowWidth / 2,
            y: y + height / 2,
            fill: "#ef4444",
            fontSize: 12,
            textAlign: "center",
            textVerticalAlign: "middle",
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
    closePanel();
    updateBreadcrumb();
    renderChart();
  }
  window.__nav = navigateTo;
  chart.on("click", (params) => {
    const node = visibleNodes[params.dataIndex];
    if (node) {
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
  window.__setRange = (global) => {
    useGlobalRange = global;
    btnFitted.classList.toggle("active", !useGlobalRange);
    btnGlobal.classList.toggle("active", useGlobalRange);
    renderChart();
  };
  window.addEventListener("resize", () => chart.resize());
  updateBreadcrumb();
  renderChart();
})();
