use std::path::Path;

use crate::error::{Error, Result};
use armitage_core::org::OrgInfo;
use armitage_labels::schema::{LabelSchema, LabelStyle, LabelStyleExample};
use armitage_sync::config::SyncConfig;
use armitage_triage::config::TriageConfig;

/// Core init logic, separated for testability.
///
/// Creates an org directory at `org_dir` with the given name and GitHub orgs.
/// Fails if `armitage.toml` already exists.
pub fn init_at(
    org_dir: &Path,
    name: &str,
    github_orgs: &[String],
    default_repo: Option<&str>,
) -> Result<()> {
    let armitage_toml = org_dir.join("armitage.toml");
    if armitage_toml.exists() {
        return Err(Error::Other(format!(
            "already initialized: {}",
            armitage_toml.display()
        )));
    }

    // Create org dir and .armitage/conflicts/ subdirectory
    std::fs::create_dir_all(org_dir.join(".armitage").join("conflicts"))?;

    // Build and write armitage.toml
    let org_info = OrgInfo {
        name: name.to_string(),
        github_orgs: github_orgs.to_vec(),
        default_repo: default_repo.map(|s| s.to_string()),
    };
    let label_schema = LabelSchema {
        prefixes: vec![],
        style: Some(LabelStyle {
            convention: "Name format: <Prefix>-<Name> where the prefix is an uppercase \
                abbreviation and the name is capitalized or hyphenated lowercase. \
                Description format: <Expanded prefix>: <description>."
                .to_string(),
            examples: vec![
                LabelStyleExample {
                    name: "A-Circuit".to_string(),
                    description: "Area: quantum circuit related issues".to_string(),
                },
                LabelStyleExample {
                    name: "P-high".to_string(),
                    description: "Priority: high priority issues".to_string(),
                },
                LabelStyleExample {
                    name: "C-bug".to_string(),
                    description: "Category: this is a bug".to_string(),
                },
            ],
        }),
    };

    let mut config = toml::Table::new();
    config.insert("org".to_string(), toml::Value::try_from(&org_info).unwrap());
    config.insert(
        "label_schema".to_string(),
        toml::Value::try_from(&label_schema).unwrap(),
    );
    config.insert(
        "sync".to_string(),
        toml::Value::try_from(SyncConfig::default()).unwrap(),
    );
    config.insert(
        "triage".to_string(),
        toml::Value::try_from(TriageConfig::default()).unwrap(),
    );
    let toml_content = toml::to_string(&config)?;
    std::fs::write(&armitage_toml, toml_content)?;

    // Write .gitignore
    let gitignore = org_dir.join(".gitignore");
    std::fs::write(gitignore, ".armitage/\n")?;

    println!("Initialized org '{}' at {}", name, org_dir.display());
    Ok(())
}

/// CLI entry point for `armitage init`.
pub fn run(name: String, github_orgs: Vec<String>, default_repo: Option<String>) -> Result<()> {
    let github_orgs = if github_orgs.is_empty() {
        vec![name.clone()]
    } else {
        github_orgs
    };
    let org_dir = std::env::current_dir()?.join(&name);
    init_at(&org_dir, &name, &github_orgs, default_repo.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn init_creates_org_directory() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path().join("myorg");
        init_at(&org, "myorg", &["myorg".to_string()], None).unwrap();

        assert!(org.exists(), "org directory should exist");
        assert!(
            org.join("armitage.toml").exists(),
            "armitage.toml should exist"
        );
        assert!(
            org.join(".armitage").join("conflicts").exists(),
            ".armitage/conflicts should exist"
        );
    }

    #[test]
    fn init_with_different_github_orgs() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path().join("myorg");
        init_at(
            &org,
            "myorg",
            &["github-org-1".to_string(), "github-org-2".to_string()],
            None,
        )
        .unwrap();

        let content = std::fs::read_to_string(org.join("armitage.toml")).unwrap();
        let raw: toml::Table = toml::from_str(&content).unwrap();
        let org_info: OrgInfo = raw.get("org").unwrap().clone().try_into().unwrap();
        assert_eq!(org_info.name, "myorg");
        assert_eq!(org_info.github_orgs, vec!["github-org-1", "github-org-2"]);
    }

    #[test]
    fn init_gitignore_contains_armitage() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path().join("myorg");
        init_at(&org, "myorg", &["myorg".to_string()], None).unwrap();

        let gitignore = std::fs::read_to_string(org.join(".gitignore")).unwrap();
        assert!(
            gitignore.contains(".armitage/"),
            ".gitignore should contain .armitage/"
        );
    }

    #[test]
    fn init_fails_if_already_exists() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path().join("myorg");
        init_at(&org, "myorg", &["myorg".to_string()], None).unwrap();

        let result = init_at(&org, "myorg", &["myorg".to_string()], None);
        assert!(result.is_err(), "second init should fail");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already initialized"),
            "error should mention 'already initialized'"
        );
    }
}
