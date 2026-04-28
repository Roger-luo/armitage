use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MilestoneFile {
    #[serde(rename = "milestone")]
    pub milestones: Vec<Milestone>,
}

impl MilestoneFile {
    pub fn empty() -> Self {
        Self { milestones: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub name: String,
    pub date: NaiveDate,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<String>,
    #[serde(rename = "type", default)]
    pub milestone_type: MilestoneType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_progress: Option<f64>,
}

impl Milestone {
    pub fn is_in_quarter(&self, year: i32, quarter: u32) -> bool {
        let q_start_month = (quarter - 1) * 3 + 1;
        let q_start = NaiveDate::from_ymd_opt(year, q_start_month, 1).unwrap();
        let q_end = if quarter == 4 {
            NaiveDate::from_ymd_opt(year, 12, 31).unwrap()
        } else {
            NaiveDate::from_ymd_opt(year, q_start_month + 3, 1)
                .unwrap()
                .pred_opt()
                .unwrap()
        };
        self.date >= q_start && self.date <= q_end
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MilestoneType {
    #[default]
    Checkpoint,
    Okr,
}

impl fmt::Display for MilestoneType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Checkpoint => write!(f, "checkpoint"),
            Self::Okr => write!(f, "okr"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_milestone_file() {
        let toml = r#"
            [[milestone]]
            name = "Alpha Release"
            date = "2025-03-31"
            description = "First alpha"
            track = "acme/project#10"
            type = "okr"
            expected_progress = 0.5

            [[milestone]]
            name = "Beta Release"
            date = "2025-06-30"
            description = "Public beta"
        "#;
        let mf: MilestoneFile = toml::from_str(toml).expect("deserialize milestone file");
        assert_eq!(mf.milestones.len(), 2);

        let alpha = &mf.milestones[0];
        assert_eq!(alpha.name, "Alpha Release");
        assert_eq!(alpha.date, NaiveDate::from_ymd_opt(2025, 3, 31).unwrap());
        assert_eq!(alpha.description, "First alpha");
        assert_eq!(alpha.track.as_deref(), Some("acme/project#10"));
        assert_eq!(alpha.milestone_type, MilestoneType::Okr);
        assert_eq!(alpha.expected_progress, Some(0.5));

        let beta = &mf.milestones[1];
        assert_eq!(beta.milestone_type, MilestoneType::Checkpoint);
        assert!(beta.track.is_none());
        assert!(beta.expected_progress.is_none());
    }

    #[test]
    fn deserialize_minimal_milestone() {
        let toml = r#"
            name = "Simple"
            date = "2025-07-04"
            description = "Just the basics"
        "#;
        let m: Milestone = toml::from_str(toml).expect("deserialize minimal milestone");
        assert_eq!(m.name, "Simple");
        assert_eq!(m.date, NaiveDate::from_ymd_opt(2025, 7, 4).unwrap());
        assert_eq!(m.description, "Just the basics");
        assert!(m.track.is_none());
        assert_eq!(m.milestone_type, MilestoneType::Checkpoint);
        assert!(m.expected_progress.is_none());
    }

    #[test]
    fn roundtrip_milestone_file() {
        let original = MilestoneFile {
            milestones: vec![
                Milestone {
                    name: "M1".to_string(),
                    date: NaiveDate::from_ymd_opt(2025, 9, 15).unwrap(),
                    description: "First milestone".to_string(),
                    track: Some("org/repo#5".to_string()),
                    milestone_type: MilestoneType::Okr,
                    expected_progress: Some(0.75),
                },
                Milestone {
                    name: "M2".to_string(),
                    date: NaiveDate::from_ymd_opt(2025, 12, 1).unwrap(),
                    description: "Second milestone".to_string(),
                    track: None,
                    milestone_type: MilestoneType::Checkpoint,
                    expected_progress: None,
                },
            ],
        };
        let serialized = toml::to_string(&original).expect("serialize milestone file");
        let deserialized: MilestoneFile =
            toml::from_str(&serialized).expect("deserialize milestone file");
        assert_eq!(deserialized.milestones.len(), 2);
        assert_eq!(deserialized.milestones[0].name, "M1");
        assert_eq!(
            deserialized.milestones[0].milestone_type,
            MilestoneType::Okr
        );
        assert_eq!(deserialized.milestones[0].expected_progress, Some(0.75));
        assert_eq!(deserialized.milestones[1].name, "M2");
        assert_eq!(
            deserialized.milestones[1].milestone_type,
            MilestoneType::Checkpoint
        );
    }

    #[test]
    fn milestone_is_in_quarter() {
        // Q1 2025: Jan–Mar
        let q1_mid = Milestone {
            name: "q1".to_string(),
            date: NaiveDate::from_ymd_opt(2025, 2, 15).unwrap(),
            description: String::new(),
            track: None,
            milestone_type: MilestoneType::Checkpoint,
            expected_progress: None,
        };
        assert!(q1_mid.is_in_quarter(2025, 1));
        assert!(!q1_mid.is_in_quarter(2025, 2));

        // Q2 2025: Apr–Jun (boundary: Jun 30)
        let q2_end = Milestone {
            name: "q2".to_string(),
            date: NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
            description: String::new(),
            track: None,
            milestone_type: MilestoneType::Checkpoint,
            expected_progress: None,
        };
        assert!(q2_end.is_in_quarter(2025, 2));
        assert!(!q2_end.is_in_quarter(2025, 3));

        // Q4 2025: Oct–Dec (boundary: Dec 31)
        let q4_last = Milestone {
            name: "q4".to_string(),
            date: NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
            description: String::new(),
            track: None,
            milestone_type: MilestoneType::Checkpoint,
            expected_progress: None,
        };
        assert!(q4_last.is_in_quarter(2025, 4));
        assert!(!q4_last.is_in_quarter(2026, 1));

        // Q3 boundary: Jul 1
        let q3_start = Milestone {
            name: "q3".to_string(),
            date: NaiveDate::from_ymd_opt(2025, 7, 1).unwrap(),
            description: String::new(),
            track: None,
            milestone_type: MilestoneType::Checkpoint,
            expected_progress: None,
        };
        assert!(q3_start.is_in_quarter(2025, 3));
        assert!(!q3_start.is_in_quarter(2025, 2));
    }
}
