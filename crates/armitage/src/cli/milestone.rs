use std::path::Path;

use chrono::NaiveDate;

use crate::error::{Error, Result};
use armitage_core::tree::find_org_root;
use armitage_milestones::milestone::{Milestone, MilestoneFile, MilestoneType};

// ---------------------------------------------------------------------------
// Public helpers used by other modules
// ---------------------------------------------------------------------------

pub fn read_milestones(org_root: &Path, node_path: &str) -> Result<MilestoneFile> {
    let path = org_root.join(node_path).join("milestones.toml");
    if !path.exists() {
        return Ok(MilestoneFile::empty());
    }
    let content = std::fs::read_to_string(&path)?;
    toml::from_str(&content)
        .map_err(|source| armitage_core::error::Error::TomlParse { path, source }.into())
}

#[allow(clippy::too_many_arguments)]
pub fn add_milestone(
    org_root: &Path,
    node_path: &str,
    name: &str,
    date: &str,
    description: &str,
    milestone_type: &str,
    expected_progress: Option<f64>,
    track: Option<&str>,
) -> Result<()> {
    // Parse date
    let parsed_date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| Error::Other(format!("invalid date: '{date}' (expected YYYY-MM-DD)")))?;

    // Parse milestone type
    let mt = parse_milestone_type(milestone_type)?;

    // Read existing milestones
    let mut mf = read_milestones(org_root, node_path)?;

    // Check for duplicate name
    if mf.milestones.iter().any(|m| m.name == name) {
        return Err(Error::Other(format!(
            "milestone '{name}' already exists on node '{node_path}'"
        )));
    }

    // Append new milestone
    mf.milestones.push(Milestone {
        name: name.to_string(),
        date: parsed_date,
        description: description.to_string(),
        track: track.map(std::string::ToString::to_string),
        milestone_type: mt,
        expected_progress,
    });

    // Write back
    let path = org_root.join(node_path).join("milestones.toml");
    let content = toml::to_string(&mf)?;
    std::fs::write(&path, content)?;

    Ok(())
}

pub fn remove_milestone(org_root: &Path, node_path: &str, name: &str) -> Result<()> {
    let mut mf = read_milestones(org_root, node_path)?;

    let before = mf.milestones.len();
    mf.milestones.retain(|m| m.name != name);
    if mf.milestones.len() == before {
        return Err(Error::Other(format!(
            "milestone '{name}' not found on node '{node_path}'"
        )));
    }

    let path = org_root.join(node_path).join("milestones.toml");
    let content = toml::to_string(&mf)?;
    std::fs::write(&path, content)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// CLI entry points
// ---------------------------------------------------------------------------

pub fn run_add(
    node_path: String,
    name: String,
    date: String,
    description: String,
    milestone_type: String,
    expected_progress: Option<f64>,
    track: Option<String>,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    // Validate that the node exists
    let node_dir = org_root.join(&node_path);
    if !node_dir.join("node.toml").exists() {
        return Err(armitage_core::error::Error::NodeNotFound(node_path).into());
    }

    add_milestone(
        &org_root,
        &node_path,
        &name,
        &date,
        &description,
        &milestone_type,
        expected_progress,
        track.as_deref(),
    )?;

    println!("Added milestone '{name}' to '{node_path}'");
    Ok(())
}

pub fn run_list(
    node_path: Option<String>,
    milestone_type: Option<String>,
    quarter: Option<String>,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    // Parse quarter filter if provided
    let quarter_filter = quarter.as_deref().map(parse_quarter).transpose()?;

    // Parse type filter if provided
    let type_filter = milestone_type
        .as_deref()
        .map(parse_milestone_type)
        .transpose()?;

    // Collect (node_path, milestones) pairs
    let entries: Vec<(String, MilestoneFile)> = if let Some(ref np) = node_path {
        vec![(np.clone(), read_milestones(&org_root, np)?)]
    } else {
        // Walk all nodes
        use armitage_core::tree::walk_nodes;
        let nodes = walk_nodes(&org_root)?;
        nodes
            .into_iter()
            .map(|e| {
                let mf = read_milestones(&org_root, &e.path)?;
                Ok((e.path, mf))
            })
            .collect::<Result<Vec<_>>>()?
    };

    for (np, mf) in &entries {
        for m in &mf.milestones {
            // Apply type filter
            if let Some(ref tf) = type_filter
                && &m.milestone_type != tf
            {
                continue;
            }
            // Apply quarter filter
            if let Some((year, q)) = quarter_filter
                && !m.is_in_quarter(year, q)
            {
                continue;
            }
            println!(
                "{:<40} {:>10}  [{}]  {}",
                np, m.date, m.milestone_type, m.name
            );
            if !m.description.is_empty() {
                println!("  {}", m.description);
            }
        }
    }

    Ok(())
}

pub fn run_remove(node_path: String, name: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    remove_milestone(&org_root, &node_path, &name)?;
    println!("Removed milestone '{name}' from '{node_path}'");
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn parse_milestone_type(s: &str) -> Result<MilestoneType> {
    match s {
        "checkpoint" => Ok(MilestoneType::Checkpoint),
        "okr" => Ok(MilestoneType::Okr),
        other => Err(Error::Other(format!(
            "unknown milestone type: '{other}' (expected 'checkpoint' or 'okr')"
        ))),
    }
}

/// Parse a quarter string like "2026-Q1" into (year, quarter).
fn parse_quarter(s: &str) -> Result<(i32, u32)> {
    let err = || Error::Other(format!("invalid quarter: '{s}' (expected YYYY-Q[1-4])"));
    let Some((year_str, q_str)) = s.split_once('-') else {
        return Err(err());
    };
    let year: i32 = year_str.parse().map_err(|_| err())?;
    let q_str = q_str.strip_prefix('Q').ok_or_else(err)?;
    let quarter: u32 = q_str.parse().map_err(|_| err())?;
    if !(1..=4).contains(&quarter) {
        return Err(err());
    }
    Ok((year, quarter))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_org(tmp: &TempDir) -> std::path::PathBuf {
        let org = tmp.path().join("testorg");
        crate::cli::init::init_at(&org, "testorg", &["testorg".to_string()], None).unwrap();
        org
    }

    fn create_test_node(org: &Path, node_path: &str) {
        let node_dir = org.join(node_path);
        std::fs::create_dir_all(&node_dir).unwrap();
        let content = format!(
            "name = \"{}\"\ndescription = \"test node\"\n",
            Path::new(node_path).file_name().unwrap().to_str().unwrap()
        );
        std::fs::write(node_dir.join("node.toml"), content).unwrap();
    }

    #[test]
    fn add_milestone_to_node() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);
        create_test_node(&org, "gemini");

        add_milestone(
            &org,
            "gemini",
            "Alpha",
            "2026-03-31",
            "First alpha release",
            "checkpoint",
            None,
            None,
        )
        .unwrap();

        let mf = read_milestones(&org, "gemini").unwrap();
        assert_eq!(mf.milestones.len(), 1);
        assert_eq!(mf.milestones[0].name, "Alpha");
        assert_eq!(
            mf.milestones[0].date,
            NaiveDate::from_ymd_opt(2026, 3, 31).unwrap()
        );
        assert_eq!(mf.milestones[0].description, "First alpha release");
        assert_eq!(mf.milestones[0].milestone_type, MilestoneType::Checkpoint);
        assert!(mf.milestones[0].expected_progress.is_none());
        assert!(mf.milestones[0].track.is_none());
    }

    #[test]
    fn add_okr_milestone() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);
        create_test_node(&org, "gemini");

        add_milestone(
            &org,
            "gemini",
            "Q1 OKR",
            "2026-03-31",
            "Quarterly objective",
            "okr",
            Some(0.75),
            Some("owner/repo#42"),
        )
        .unwrap();

        let mf = read_milestones(&org, "gemini").unwrap();
        assert_eq!(mf.milestones.len(), 1);
        let m = &mf.milestones[0];
        assert_eq!(m.milestone_type, MilestoneType::Okr);
        assert_eq!(m.expected_progress, Some(0.75));
        assert_eq!(m.track.as_deref(), Some("owner/repo#42"));
    }

    #[test]
    fn add_multiple_milestones() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);
        create_test_node(&org, "gemini");

        add_milestone(
            &org,
            "gemini",
            "M1",
            "2026-01-31",
            "First",
            "checkpoint",
            None,
            None,
        )
        .unwrap();
        add_milestone(
            &org,
            "gemini",
            "M2",
            "2026-06-30",
            "Second",
            "checkpoint",
            None,
            None,
        )
        .unwrap();
        add_milestone(
            &org,
            "gemini",
            "M3",
            "2026-09-30",
            "Third",
            "okr",
            Some(0.5),
            None,
        )
        .unwrap();

        let mf = read_milestones(&org, "gemini").unwrap();
        assert_eq!(mf.milestones.len(), 3);
        assert_eq!(mf.milestones[0].name, "M1");
        assert_eq!(mf.milestones[1].name, "M2");
        assert_eq!(mf.milestones[2].name, "M3");
    }

    #[test]
    fn remove_milestone_test() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);
        create_test_node(&org, "gemini");

        add_milestone(
            &org,
            "gemini",
            "M1",
            "2026-01-31",
            "First",
            "checkpoint",
            None,
            None,
        )
        .unwrap();
        add_milestone(
            &org,
            "gemini",
            "M2",
            "2026-06-30",
            "Second",
            "checkpoint",
            None,
            None,
        )
        .unwrap();

        super::remove_milestone(&org, "gemini", "M1").unwrap();

        let mf = read_milestones(&org, "gemini").unwrap();
        assert_eq!(mf.milestones.len(), 1);
        assert_eq!(mf.milestones[0].name, "M2");
    }

    #[test]
    fn remove_nonexistent_milestone_errors() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);
        create_test_node(&org, "gemini");

        let result = super::remove_milestone(&org, "gemini", "DoesNotExist");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("DoesNotExist"),
            "error should mention the milestone name"
        );
    }
}
