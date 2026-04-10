use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};

use crate::error::Result;
use armitage_chart::data::IssueDates;
use armitage_core::org::Org;
use armitage_core::tree::{find_org_root, walk_nodes};

pub fn run_chart(output: Option<String>, no_open: bool, offline: bool, watch: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    let output_path = output
        .map(PathBuf::from)
        .unwrap_or_else(|| org_root.join(".armitage").join("chart.html"));

    generate_chart(&org_root, &output_path, offline)?;

    if !no_open {
        open_in_browser(&output_path);
    }

    if watch {
        run_watch(&org_root, &output_path, offline)?;
    }

    Ok(())
}

fn generate_chart(org_root: &Path, output_path: &Path, offline: bool) -> Result<()> {
    let org = Org::open(org_root)?;
    let entries = walk_nodes(org_root)?;
    let issue_dates = build_issue_dates_map(org_root);

    let chart_data = armitage_chart::build_chart_data(&entries, &org.info().name, &issue_dates)?;
    let html = armitage_chart::render_chart(&chart_data, offline)?;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, &html)?;
    eprintln!("Chart written to {}", output_path.display());
    Ok(())
}

fn run_watch(org_root: &Path, output_path: &Path, offline: bool) -> Result<()> {
    eprintln!("Watching for changes... (press Ctrl+C to stop)");

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .map_err(|e| crate::error::Error::Other(format!("watch error: {e}")))?;

    // Watch the org root for toml/issues changes
    watcher
        .watch(org_root, RecursiveMode::Recursive)
        .map_err(|e| crate::error::Error::Other(format!("watch error: {e}")))?;

    // Also watch the triage DB directory
    let triage_dir = org_root.join(".armitage").join("triage");
    if triage_dir.exists() {
        let _ = watcher.watch(&triage_dir, RecursiveMode::NonRecursive);
    }

    let mut last_rebuild = Instant::now();
    let debounce = Duration::from_millis(500);

    while let Ok(event) = rx.recv() {
        if !is_relevant_change(&event, output_path) {
            continue;
        }
        // Debounce: skip if we just rebuilt
        if last_rebuild.elapsed() < debounce {
            while rx.try_recv().is_ok() {}
            continue;
        }
        // Small delay to let writes settle
        std::thread::sleep(Duration::from_millis(200));
        while rx.try_recv().is_ok() {}

        match generate_chart(org_root, output_path, offline) {
            Ok(()) => {
                last_rebuild = Instant::now();
            }
            Err(e) => {
                eprintln!("  rebuild error: {e}");
            }
        }
    }

    Ok(())
}

fn is_relevant_change(event: &notify::Event, output_path: &Path) -> bool {
    // Only care about creates, modifications, and renames
    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return false;
    }

    event.paths.iter().any(|p| {
        // Skip changes to the output file itself
        if p == output_path {
            return false;
        }
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Watch: node.toml, issues.toml, milestones.toml, armitage.toml, triage.db, labels.toml
        name == "node.toml"
            || name == "issues.toml"
            || name == "milestones.toml"
            || name == "armitage.toml"
            || name == "labels.toml"
            || name == "triage.db"
            || name == "team.toml"
    })
}

/// Build a map of issue_ref -> IssueDates from the triage DB's project items.
fn build_issue_dates_map(org_root: &Path) -> HashMap<String, IssueDates> {
    let mut map = HashMap::new();
    let Ok(conn) = armitage_triage::db::open_db(org_root) else {
        return map;
    };
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

fn open_in_browser(path: &Path) {
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
