use armitage_core::org::Org;

use crate::error::Result;

pub fn run_sync(dry_run: bool) -> Result<()> {
    let org = Org::discover_from(&std::env::current_dir()?)?;
    let gh = armitage_github::require_gh()?;

    let stats = armitage_project::sync(&gh, &org, dry_run)?;

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
