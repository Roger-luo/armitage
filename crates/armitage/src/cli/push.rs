use crate::cli::util;
use crate::error::Result;

pub fn run(path: Option<String>, dry_run: bool) -> Result<()> {
    let org_root = util::org_root()?;
    let gh = armitage_github::require_gh()?;
    armitage_sync::push::push_all(&gh, &org_root, path.as_deref(), dry_run)?;
    Ok(())
}
