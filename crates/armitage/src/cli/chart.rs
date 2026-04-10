use crate::error::Result;
use armitage_core::org::Org;
use armitage_core::tree::{find_org_root, walk_nodes};
use std::path::PathBuf;

pub fn run_chart(output: Option<String>, no_open: bool, offline: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let org = Org::open(&org_root)?;
    let entries = walk_nodes(&org_root)?;

    let chart_data = armitage_chart::build_chart_data(&entries, &org.info().name)?;
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
