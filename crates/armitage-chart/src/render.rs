use askama::Template;

use crate::data::ChartData;
use crate::error::Result;

const CHART_JS: &str = include_str!("../js/chart.js");
const ECHARTS_JS: &str = include_str!("../js/echarts.min.js");

#[derive(Template)]
#[template(path = "chart.html")]
struct ChartTemplate {
    title: String,
    chart_data_json: String,
    chart_js: &'static str,
    inline_js: bool,
    echart_js: String,
}

/// Render chart data into a standalone HTML string.
///
/// When `offline` is true, ECharts JS is embedded inline so the chart works
/// without network access.
pub fn render_chart(data: &ChartData, offline: bool) -> Result<String> {
    let chart_data_json = serde_json::to_string(data)?;

    let template = ChartTemplate {
        title: data.org_name.clone(),
        chart_data_json,
        chart_js: CHART_JS,
        inline_js: offline,
        echart_js: if offline {
            ECHARTS_JS.to_string()
        } else {
            String::new()
        },
    };

    Ok(template.render()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{ChartData, ChartNode};

    fn sample_data() -> ChartData {
        ChartData {
            nodes: vec![ChartNode {
                path: "alpha".to_string(),
                name: "Alpha".to_string(),
                description: "Test initiative".to_string(),
                status: "active".to_string(),
                start: Some("2026-01-01".to_string()),
                end: Some("2026-06-30".to_string()),
                eff_start: Some("2026-01-01".to_string()),
                eff_end: Some("2026-06-30".to_string()),
                has_timeline: true,
                owners: vec!["alice".to_string()],
                team: Some("core".to_string()),
                children: vec![],
                milestones: vec![],
                issues: vec![],
                overflow_start: None,
                overflow_end: None,
                issue_start: None,
                issue_end: None,
            }],
            org_name: "TestOrg".to_string(),
            global_start: Some("2026-01-01".to_string()),
            global_end: Some("2026-06-30".to_string()),
        }
    }

    #[test]
    fn renders_valid_html() {
        let html = render_chart(&sample_data(), false).unwrap();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("TestOrg"));
        assert!(html.contains("cdn.jsdelivr.net/npm/echarts"));
        assert!(html.contains("__CHART_DATA__"));
        assert!(html.contains("Alpha"));
    }

    #[test]
    fn cdn_mode_includes_script_tag() {
        let html = render_chart(&sample_data(), false).unwrap();
        assert!(html.contains("cdn.jsdelivr.net/npm/echarts@5"));
    }
}
