use crate::error::Result;
use armitage_core::tree::find_org_root;

pub fn run(path: Option<String>, dry_run: bool) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let gh = armitage_github::require_gh()?;
    armitage_sync::push::push_all(&gh, &org_root, path.as_deref(), dry_run)?;
    Ok(())
}
