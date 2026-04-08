use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

fn secrets_path(org_root: &Path) -> PathBuf {
    org_root.join(".armitage").join("secrets.toml")
}

pub fn read_secret(org_root: &Path, key: &str) -> Result<Option<String>> {
    let path = secrets_path(org_root);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let table: toml::Table =
        toml::from_str(&content).map_err(|source| Error::TomlParse { path, source })?;
    Ok(table.get(key).and_then(|v| v.as_str()).map(String::from))
}

pub fn write_secret(org_root: &Path, key: &str, value: &str) -> Result<()> {
    let path = secrets_path(org_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Verify .armitage/ is gitignored before writing secrets
    ensure_gitignored(org_root)?;

    let mut table: toml::Table = if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        toml::from_str(&content).map_err(|source| Error::TomlParse {
            path: path.clone(),
            source,
        })?
    } else {
        toml::Table::new()
    };

    table.insert(key.to_string(), toml::Value::String(value.to_string()));
    let content = toml::to_string(&table)?;
    std::fs::write(&path, content)?;

    // Restrict to owner read/write only
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Ensure `.armitage/` is gitignored. Checks .gitignore and adds to
/// `.git/info/exclude` as a second layer of protection.
fn ensure_gitignored(org_root: &Path) -> Result<()> {
    let gitignore = org_root.join(".gitignore");
    let has_gitignore_entry = gitignore
        .exists()
        .then(|| std::fs::read_to_string(&gitignore).ok())
        .flatten()
        .is_some_and(|content| {
            content
                .lines()
                .any(|line| line.trim() == ".armitage/" || line.trim() == ".armitage")
        });

    if !has_gitignore_entry {
        return Err(Error::Other(
            ".armitage/ is not in .gitignore — refusing to write secrets. \
             Add '.armitage/' to .gitignore first."
                .to_string(),
        ));
    }

    // Belt-and-suspenders: also add to .git/info/exclude if a git repo exists
    let exclude = org_root.join(".git").join("info").join("exclude");
    if let Some(parent) = exclude.parent()
        && parent.exists()
    {
        let needs_entry = exclude
            .exists()
            .then(|| std::fs::read_to_string(&exclude).ok())
            .flatten()
            .is_none_or(|content| {
                !content
                    .lines()
                    .any(|line| line.trim() == ".armitage/" || line.trim() == ".armitage")
            });
        if needs_entry {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&exclude)?;
            std::io::Write::write_all(&mut f, b"\n.armitage/\n")?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Create a temp dir with a .gitignore that covers .armitage/.
    fn tmp_with_gitignore() -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), ".armitage/\n").unwrap();
        tmp
    }

    #[test]
    fn roundtrip_secret() {
        let tmp = tmp_with_gitignore();
        assert_eq!(read_secret(tmp.path(), "test-key").unwrap(), None);

        write_secret(tmp.path(), "test-key", "secret-value").unwrap();
        assert_eq!(
            read_secret(tmp.path(), "test-key").unwrap().as_deref(),
            Some("secret-value")
        );
    }

    #[test]
    fn multiple_secrets() {
        let tmp = tmp_with_gitignore();
        write_secret(tmp.path(), "key-a", "val-a").unwrap();
        write_secret(tmp.path(), "key-b", "val-b").unwrap();

        assert_eq!(
            read_secret(tmp.path(), "key-a").unwrap().as_deref(),
            Some("val-a")
        );
        assert_eq!(
            read_secret(tmp.path(), "key-b").unwrap().as_deref(),
            Some("val-b")
        );
    }

    #[cfg(unix)]
    #[test]
    fn secret_file_has_restricted_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tmp_with_gitignore();
        write_secret(tmp.path(), "key", "val").unwrap();
        let perms = std::fs::metadata(secrets_path(tmp.path()))
            .unwrap()
            .permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[test]
    fn refuses_without_gitignore() {
        let tmp = TempDir::new().unwrap();
        let err = write_secret(tmp.path(), "key", "val")
            .unwrap_err()
            .to_string();
        assert!(err.contains(".gitignore"));
    }
}
