use std::collections::HashMap;

use crate::error::Result;
use armitage_chart::data::IssueDates;
use armitage_core::org::Org;
use armitage_core::tree::{find_org_root, walk_nodes};
use std::path::PathBuf;

pub fn run_chart(output: Option<String>, no_open: bool, offline: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let org = Org::open(&org_root)?;
    let entries = walk_nodes(&org_root)?;

    // Build issue dates map from triage DB (if available)
    let issue_dates = build_issue_dates_map(&org_root);

    let chart_data = armitage_chart::build_chart_data(&entries, &org.info().name, &issue_dates)?;
    let html = armitage_chart::render_chart(&chart_data, offline)?;

    let output_path = output
        .map(PathBuf::from)
        .unwrap_or_else(|| org_root.join(".armitage").join("chart.html"));

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output_path, &html)?;
    eprintln!("Chart written to {}", output_path.display());

    if !no_open {
        open_in_browser(&output_path);
    }
    Ok(())
}

/// Build a map of issue_ref -> IssueDates from the triage DB's project items.
fn build_issue_dates_map(org_root: &std::path::Path) -> HashMap<String, IssueDates> {
    let mut map = HashMap::new();
    let Ok(conn) = armitage_triage::db::open_db(org_root) else {
        return map;
    };
    // Query all project items joined with issues to get repo#number -> dates
    let Ok(mut stmt) = conn.prepare(
        "SELECT i.repo, i.number, p.start_date, p.target_date
         FROM issue_project_items p
         JOIN issues i ON i.id = p.issue_id
         WHERE p.start_date IS NOT NULL OR p.target_date IS NOT NULL",
    ) else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        let repo: String = row.get(0)?;
        let number: i64 = row.get(1)?;
        let start_date: Option<String> = row.get(2)?;
        let target_date: Option<String> = row.get(3)?;
        Ok((format!("{repo}#{number}"), start_date, target_date))
    }) else {
        return map;
    };
    for row in rows.flatten() {
        let (issue_ref, start_date, target_date) = row;
        map.insert(
            issue_ref,
            IssueDates {
                start_date,
                target_date,
            },
        );
    }
    map
}

fn open_in_browser(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(target_os = "windows")]
    let cmd = "cmd";

    #[cfg(target_os = "windows")]
    let args = vec!["/c", "start", &path.display().to_string()];
    #[cfg(not(target_os = "windows"))]
    let args = vec![path.to_str().unwrap_or_default()];

    let _ = std::process::Command::new(cmd).args(&args).spawn();
}
