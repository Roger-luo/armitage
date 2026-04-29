use armitage_core::org::Org;
use chrono::NaiveDate;

use crate::error::Result;

pub fn run_set(
    issue: String,
    start_date: Option<String>,
    target_date: Option<String>,
    dry_run: bool,
) -> Result<()> {
    let org = Org::discover_from(&std::env::current_dir()?)?;
    let gh = armitage_github::require_gh()?;

    let parse_date = |s: &str| {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
            crate::error::Error::Other(format!("invalid date '{s}', expected YYYY-MM-DD"))
        })
    };

    let start = start_date.as_deref().map(parse_date).transpose()?;
    let target = target_date.as_deref().map(parse_date).transpose()?;

    if start.is_none() && target.is_none() {
        return Err(crate::error::Error::Other(
            "at least one of --start-date or --target-date is required".into(),
        ));
    }

    armitage_project::set_issue(&gh, &org, &issue, start, target, dry_run)?;
    Ok(())
}

pub fn run_sync(dry_run: bool, node_path: Option<String>) -> Result<()> {
    let org = Org::discover_from(&std::env::current_dir()?)?;
    let gh = armitage_github::require_gh()?;

    let stats = armitage_project::sync(&gh, &org, dry_run, node_path.as_deref())?;

    println!();
    println!(
        "Done: {} added, {} updated, {} skipped, {} errors",
        stats.added, stats.updated, stats.skipped, stats.errors
    );
    Ok(())
}

pub fn run_clear_cache() -> Result<()> {
    let org = Org::discover_from(&std::env::current_dir()?)?;
    let cache_path = org
        .root()
        .join(".armitage")
        .join("project")
        .join("field-cache.toml");
    if cache_path.exists() {
        std::fs::remove_file(&cache_path)?;
        println!("Cleared project field cache.");
    } else {
        println!("No cache file found.");
    }
    Ok(())
}
