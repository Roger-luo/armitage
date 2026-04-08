use std::path::Path;

/// Migrate `.armitage/` from the old flat layout to namespaced domain directories.
///
/// Old layout:
///   .armitage/sync-state.toml, .armitage/conflicts/, .armitage/triage.db, etc.
///
/// New layout:
///   .armitage/sync/state.toml, .armitage/sync/conflicts/,
///   .armitage/triage/triage.db, .armitage/labels/renames.toml, etc.
pub fn migrate_dotarmitage(org_root: &Path) -> std::io::Result<()> {
    let armitage_dir = org_root.join(".armitage");
    if !armitage_dir.exists() {
        return Ok(());
    }

    // Sync state
    let old_sync_state = armitage_dir.join("sync-state.toml");
    let new_sync_dir = armitage_dir.join("sync");
    if old_sync_state.exists() && !new_sync_dir.join("state.toml").exists() {
        std::fs::create_dir_all(&new_sync_dir)?;
        std::fs::rename(&old_sync_state, new_sync_dir.join("state.toml"))?;
    }

    // Conflicts
    let old_conflicts = armitage_dir.join("conflicts");
    let new_conflicts = armitage_dir.join("sync").join("conflicts");
    if old_conflicts.exists() && !new_conflicts.exists() {
        std::fs::create_dir_all(new_conflicts.parent().unwrap())?;
        std::fs::rename(&old_conflicts, &new_conflicts)?;
    }

    // Triage DB (old name was issues.db)
    let old_db = armitage_dir.join("issues.db");
    let new_triage_dir = armitage_dir.join("triage");
    if old_db.exists() && !new_triage_dir.join("triage.db").exists() {
        std::fs::create_dir_all(&new_triage_dir)?;
        std::fs::rename(&old_db, new_triage_dir.join("triage.db"))?;
    }

    // Label renames
    let old_renames = armitage_dir.join("label-renames.toml");
    let new_labels_dir = armitage_dir.join("labels");
    if old_renames.exists() && !new_labels_dir.join("renames.toml").exists() {
        std::fs::create_dir_all(&new_labels_dir)?;
        std::fs::rename(&old_renames, new_labels_dir.join("renames.toml"))?;
    }

    // Label import sessions
    let old_imports = armitage_dir.join("label-imports");
    let new_imports = armitage_dir.join("triage").join("label-imports");
    if old_imports.exists() && !new_imports.exists() {
        std::fs::create_dir_all(&new_triage_dir)?;
        std::fs::rename(&old_imports, &new_imports)?;
    }

    // Triage examples
    let old_examples = armitage_dir.join("triage-examples.toml");
    if old_examples.exists() && !new_triage_dir.join("examples.toml").exists() {
        std::fs::create_dir_all(&new_triage_dir)?;
        std::fs::rename(&old_examples, new_triage_dir.join("examples.toml"))?;
    }

    // Dismissed categories
    let old_dismissed = armitage_dir.join("dismissed-categories.toml");
    if old_dismissed.exists() && !new_triage_dir.join("dismissed-categories.toml").exists() {
        std::fs::create_dir_all(&new_triage_dir)?;
        std::fs::rename(
            &old_dismissed,
            new_triage_dir.join("dismissed-categories.toml"),
        )?;
    }

    // Issue/repo cache
    let old_cache = armitage_dir.join("issue-cache");
    if old_cache.exists() && !new_triage_dir.join("repo-cache").exists() {
        std::fs::create_dir_all(&new_triage_dir)?;
        std::fs::rename(&old_cache, new_triage_dir.join("repo-cache"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn migrate_noop_when_no_dotarmitage() {
        let tmp = TempDir::new().unwrap();
        assert!(migrate_dotarmitage(tmp.path()).is_ok());
    }

    #[test]
    fn migrate_moves_sync_state() {
        let tmp = TempDir::new().unwrap();
        let dot = tmp.path().join(".armitage");
        std::fs::create_dir_all(&dot).unwrap();
        std::fs::write(dot.join("sync-state.toml"), "old").unwrap();

        migrate_dotarmitage(tmp.path()).unwrap();

        assert!(!dot.join("sync-state.toml").exists());
        assert_eq!(
            std::fs::read_to_string(dot.join("sync").join("state.toml")).unwrap(),
            "old"
        );
    }

    #[test]
    fn migrate_moves_conflicts() {
        let tmp = TempDir::new().unwrap();
        let dot = tmp.path().join(".armitage");
        std::fs::create_dir_all(dot.join("conflicts")).unwrap();
        std::fs::write(dot.join("conflicts").join("a.toml"), "c").unwrap();

        migrate_dotarmitage(tmp.path()).unwrap();

        assert!(!dot.join("conflicts").exists());
        assert_eq!(
            std::fs::read_to_string(dot.join("sync").join("conflicts").join("a.toml")).unwrap(),
            "c"
        );
    }

    #[test]
    fn migrate_moves_triage_db() {
        let tmp = TempDir::new().unwrap();
        let dot = tmp.path().join(".armitage");
        std::fs::create_dir_all(&dot).unwrap();
        std::fs::write(dot.join("issues.db"), "db").unwrap();

        migrate_dotarmitage(tmp.path()).unwrap();

        assert!(!dot.join("issues.db").exists());
        assert_eq!(
            std::fs::read_to_string(dot.join("triage").join("triage.db")).unwrap(),
            "db"
        );
    }

    #[test]
    fn migrate_moves_label_renames() {
        let tmp = TempDir::new().unwrap();
        let dot = tmp.path().join(".armitage");
        std::fs::create_dir_all(&dot).unwrap();
        std::fs::write(dot.join("label-renames.toml"), "renames").unwrap();

        migrate_dotarmitage(tmp.path()).unwrap();

        assert!(!dot.join("label-renames.toml").exists());
        assert_eq!(
            std::fs::read_to_string(dot.join("labels").join("renames.toml")).unwrap(),
            "renames"
        );
    }

    #[test]
    fn migrate_skips_when_new_layout_exists() {
        let tmp = TempDir::new().unwrap();
        let dot = tmp.path().join(".armitage");
        std::fs::create_dir_all(dot.join("sync")).unwrap();
        std::fs::write(dot.join("sync-state.toml"), "old").unwrap();
        std::fs::write(dot.join("sync").join("state.toml"), "new").unwrap();

        migrate_dotarmitage(tmp.path()).unwrap();

        // Old file should remain untouched (migration skipped)
        assert!(dot.join("sync-state.toml").exists());
        assert_eq!(
            std::fs::read_to_string(dot.join("sync").join("state.toml")).unwrap(),
            "new"
        );
    }
}
