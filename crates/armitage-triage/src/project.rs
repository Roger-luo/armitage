use std::collections::HashMap;

use rusqlite::Connection;
use serde::Deserialize;

use crate::config::ProjectConfig;
use crate::db::{self, IssueProjectItem};
use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// URL parsing
// ---------------------------------------------------------------------------

/// Extract the organization name and project number from a GitHub Projects v2 URL.
///
/// Expected format: `https://github.com/orgs/<org>/projects/<number>`
pub fn parse_project_url(url: &str) -> Result<(String, u32)> {
    let url = url.trim_end_matches('/');
    let parts: Vec<&str> = url.split('/').collect();

    // Walk backwards to find "orgs" and extract the org name.
    let orgs_idx = parts.iter().position(|&p| p == "orgs").ok_or_else(|| {
        Error::Other(format!("expected '/orgs/<org>/projects/<n>' in URL: {url}"))
    })?;

    let org = parts
        .get(orgs_idx + 1)
        .ok_or_else(|| Error::Other(format!("missing org name after '/orgs/' in URL: {url}")))?;

    let number_str = parts
        .last()
        .ok_or_else(|| Error::Other(format!("invalid project URL (too few segments): {url}")))?;
    let number: u32 = number_str.parse().map_err(|_| {
        Error::Other(format!(
            "invalid project number '{number_str}' in URL: {url}"
        ))
    })?;

    Ok(((*org).to_string(), number))
}

// ---------------------------------------------------------------------------
// GraphQL query
// ---------------------------------------------------------------------------

const PROJECT_ITEMS_QUERY: &str = r"
query($org: String!, $number: Int!, $cursor: String) {
  organization(login: $org) {
    projectV2(number: $number) {
      items(first: 100, after: $cursor) {
        pageInfo {
          hasNextPage
          endCursor
        }
        nodes {
          content {
            ... on Issue {
              number
              repository {
                nameWithOwner
              }
            }
            ... on PullRequest {
              number
              repository {
                nameWithOwner
              }
            }
          }
          fieldValues(first: 20) {
            nodes {
              ... on ProjectV2ItemFieldTextValue {
                text
                field { ... on ProjectV2Field { name } }
              }
              ... on ProjectV2ItemFieldNumberValue {
                number
                field { ... on ProjectV2Field { name } }
              }
              ... on ProjectV2ItemFieldDateValue {
                date
                field { ... on ProjectV2Field { name } }
              }
              ... on ProjectV2ItemFieldSingleSelectValue {
                name
                field { ... on ProjectV2SingleSelectField { name } }
              }
              ... on ProjectV2ItemFieldIterationValue {
                title
                field { ... on ProjectV2IterationField { name } }
              }
            }
          }
        }
      }
    }
  }
}
";

// ---------------------------------------------------------------------------
// GraphQL response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GqlResponse {
    data: Option<GqlData>,
    errors: Option<Vec<GqlError>>,
}

#[derive(Debug, Deserialize)]
struct GqlError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct GqlData {
    organization: Option<GqlOrganization>,
}

#[derive(Debug, Deserialize)]
struct GqlOrganization {
    #[serde(rename = "projectV2")]
    project_v2: Option<GqlProjectV2>,
}

#[derive(Debug, Deserialize)]
struct GqlProjectV2 {
    items: GqlItemConnection,
}

#[derive(Debug, Deserialize)]
struct GqlItemConnection {
    #[serde(rename = "pageInfo")]
    page_info: GqlPageInfo,
    nodes: Vec<GqlProjectItem>,
}

#[derive(Debug, Deserialize)]
struct GqlPageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GqlProjectItem {
    content: Option<GqlContent>,
    #[serde(rename = "fieldValues")]
    field_values: GqlFieldValues,
}

#[derive(Debug, Deserialize)]
struct GqlContent {
    number: Option<u64>,
    repository: Option<GqlRepository>,
}

#[derive(Debug, Deserialize)]
struct GqlRepository {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

#[derive(Debug, Deserialize)]
struct GqlFieldValues {
    nodes: Vec<GqlFieldValue>,
}

/// Union of all ProjectV2ItemField*Value types we care about.
/// Each variant contributes at most one value field plus a `field.name`.
#[derive(Debug, Deserialize)]
struct GqlFieldValue {
    // DateValue
    #[serde(default)]
    date: Option<String>,
    // TextValue
    #[serde(default)]
    text: Option<String>,
    // NumberValue
    #[serde(default)]
    number: Option<f64>,
    // SingleSelectValue — the selected option name
    #[serde(default)]
    name: Option<String>,
    // IterationValue
    #[serde(default)]
    title: Option<String>,
    // Nested field name (present on all typed fragments)
    #[serde(default)]
    field: Option<GqlFieldRef>,
}

#[derive(Debug, Deserialize)]
struct GqlFieldRef {
    #[serde(default)]
    name: Option<String>,
}

// ---------------------------------------------------------------------------
// GraphQL execution
// ---------------------------------------------------------------------------

/// Execute the project-items GraphQL query for one page.
fn execute_graphql_query(
    gh: &armitage_github::Gh,
    org: &str,
    number: u32,
    cursor: Option<&str>,
) -> Result<String> {
    let query_arg = format!("query={PROJECT_ITEMS_QUERY}");
    let org_arg = format!("org={org}");
    let number_arg = format!("number={number}");

    let mut args: Vec<&str> = vec![
        "api",
        "graphql",
        "-f",
        &query_arg,
        "-F",
        &org_arg,
        "-F",
        &number_arg,
    ];

    let cursor_arg;
    if let Some(c) = cursor {
        cursor_arg = format!("cursor={c}");
        args.push("-f");
        args.push(&cursor_arg);
    }

    let output = gh.run(&args).map_err(armitage_github::error::Error::from)?;
    Ok(output)
}

// ---------------------------------------------------------------------------
// Field extraction helpers
// ---------------------------------------------------------------------------

/// Extract a string value from a field-value node, trying all typed fields.
fn field_value_string(fv: &GqlFieldValue) -> Option<String> {
    if let Some(ref d) = fv.date {
        return Some(d.clone());
    }
    if let Some(ref t) = fv.text {
        return Some(t.clone());
    }
    if let Some(n) = fv.number {
        return Some(n.to_string());
    }
    if let Some(ref n) = fv.name {
        return Some(n.clone());
    }
    if let Some(ref t) = fv.title {
        return Some(t.clone());
    }
    None
}

/// Build a map of `field display name -> string value` from a list of field-value nodes.
fn extract_fields(field_values: &[GqlFieldValue]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for fv in field_values {
        if let Some(ref field_ref) = fv.field
            && let Some(ref field_name) = field_ref.name
            && let Some(val) = field_value_string(fv)
        {
            map.insert(field_name.clone(), val);
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Map a project item to a DB record
// ---------------------------------------------------------------------------

/// Try to convert a single GraphQL project item into an `IssueProjectItem`.
///
/// Returns `None` for draft items (content is null), PRs (not in issues table),
/// or issues not yet fetched into the local DB.
fn map_item_to_project_record(
    conn: &Connection,
    item: &GqlProjectItem,
    field_map: &HashMap<String, String>,
    project_url: &str,
    now: &str,
) -> Result<Option<IssueProjectItem>> {
    // Draft items have content: null — skip.
    let Some(content) = &item.content else {
        return Ok(None);
    };

    let Some(number) = content.number else {
        return Ok(None);
    };

    let Some(repo) = &content.repository else {
        return Ok(None);
    };
    let repo = &repo.name_with_owner;

    // Look up the issue in our DB; skip if not found (could be a PR or unfetched issue).
    let Some(issue_id) = db::lookup_issue_id(conn, repo, number)? else {
        return Ok(None);
    };

    let fields = extract_fields(&item.field_values.nodes);

    let target_date = field_map
        .get("target_date")
        .and_then(|display| fields.get(display))
        .cloned();

    let start_date = field_map
        .get("start_date")
        .and_then(|display| fields.get(display))
        .cloned();

    let status = field_map
        .get("status")
        .and_then(|display| fields.get(display))
        .cloned();

    Ok(Some(IssueProjectItem {
        id: 0,
        issue_id,
        project_url: project_url.to_string(),
        target_date,
        start_date,
        status,
        fetched_at: now.to_string(),
    }))
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Fetch all project items from a GitHub Projects v2 board and upsert them
/// into the `issue_project_items` table.
///
/// Returns the number of items upserted.
pub fn fetch_project_items(
    gh: &armitage_github::Gh,
    conn: &Connection,
    config: &ProjectConfig,
) -> Result<usize> {
    let (org, number) = parse_project_url(&config.url)?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut cursor: Option<String> = None;
    let mut total = 0usize;

    loop {
        let json = execute_graphql_query(gh, &org, number, cursor.as_deref())?;
        let resp: GqlResponse = serde_json::from_str(&json)?;

        if let Some(errors) = &resp.errors {
            let msgs: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
            return Err(Error::Other(format!("GraphQL errors: {}", msgs.join("; "))));
        }

        let items_connection = resp
            .data
            .as_ref()
            .and_then(|d| d.organization.as_ref())
            .and_then(|o| o.project_v2.as_ref())
            .map(|p| &p.items)
            .ok_or_else(|| {
                Error::Other(format!(
                    "project not found: org={org} number={number}. \
                     Check the URL and that `gh` has the project:read scope."
                ))
            })?;

        for item in &items_connection.nodes {
            if let Some(record) =
                map_item_to_project_record(conn, item, &config.fields, &config.url, &now)?
            {
                db::upsert_project_item(conn, &record)?;
                total += 1;
            }
        }

        if items_connection.page_info.has_next_page {
            match &items_connection.page_info.end_cursor {
                Some(c) => cursor = Some(c.clone()),
                None => break, // no cursor to advance — stop to avoid infinite loop
            }
        } else {
            break;
        }
    }

    Ok(total)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_project_url_valid() {
        let (org, num) = parse_project_url("https://github.com/orgs/MyOrg/projects/42").unwrap();
        assert_eq!(org, "MyOrg");
        assert_eq!(num, 42);
    }

    #[test]
    fn parse_project_url_trailing_slash() {
        let (org, num) =
            parse_project_url("https://github.com/orgs/QuEraComputing/projects/7/").unwrap();
        assert_eq!(org, "QuEraComputing");
        assert_eq!(num, 7);
    }

    #[test]
    fn parse_project_url_invalid_number() {
        assert!(parse_project_url("https://github.com/orgs/Org/projects/abc").is_err());
    }

    #[test]
    fn parse_project_url_missing_orgs() {
        assert!(parse_project_url("https://github.com/users/Org/projects/1").is_err());
    }

    #[test]
    fn extract_fields_date_and_select() {
        let fvs = vec![
            GqlFieldValue {
                date: Some("2026-06-01".to_string()),
                text: None,
                number: None,
                name: None,
                title: None,
                field: Some(GqlFieldRef {
                    name: Some("Target date".to_string()),
                }),
            },
            GqlFieldValue {
                date: None,
                text: None,
                number: None,
                name: Some("In Progress".to_string()),
                title: None,
                field: Some(GqlFieldRef {
                    name: Some("Status".to_string()),
                }),
            },
        ];
        let map = extract_fields(&fvs);
        assert_eq!(map.get("Target date").unwrap(), "2026-06-01");
        assert_eq!(map.get("Status").unwrap(), "In Progress");
    }

    #[test]
    fn field_value_string_priority() {
        // date wins over text
        let fv = GqlFieldValue {
            date: Some("2026-01-01".to_string()),
            text: Some("hello".to_string()),
            number: None,
            name: None,
            title: None,
            field: None,
        };
        assert_eq!(field_value_string(&fv).unwrap(), "2026-01-01");
    }

    #[test]
    fn gql_response_deser_with_items() {
        let json = r#"{
            "data": {
                "organization": {
                    "projectV2": {
                        "items": {
                            "pageInfo": { "hasNextPage": false, "endCursor": null },
                            "nodes": [
                                {
                                    "content": {
                                        "number": 42,
                                        "repository": { "nameWithOwner": "org/repo" }
                                    },
                                    "fieldValues": {
                                        "nodes": [
                                            {
                                                "date": "2026-05-01",
                                                "field": { "name": "Target date" }
                                            }
                                        ]
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        }"#;
        let resp: GqlResponse = serde_json::from_str(json).unwrap();
        let data = resp.data.unwrap();
        let org = data.organization.unwrap();
        let proj = org.project_v2.unwrap();
        assert_eq!(proj.items.nodes.len(), 1);
        assert!(!proj.items.page_info.has_next_page);
    }

    #[test]
    fn gql_response_deser_draft_item() {
        let json = r#"{
            "data": {
                "organization": {
                    "projectV2": {
                        "items": {
                            "pageInfo": { "hasNextPage": false, "endCursor": null },
                            "nodes": [
                                {
                                    "content": null,
                                    "fieldValues": { "nodes": [] }
                                }
                            ]
                        }
                    }
                }
            }
        }"#;
        let resp: GqlResponse = serde_json::from_str(json).unwrap();
        let items = &resp
            .data
            .unwrap()
            .organization
            .unwrap()
            .project_v2
            .unwrap()
            .items;
        assert_eq!(items.nodes.len(), 1);
        assert!(items.nodes[0].content.is_none());
    }
}
