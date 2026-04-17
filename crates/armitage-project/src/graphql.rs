use std::collections::HashMap;

use armitage_github::Gh;
use serde::Deserialize;

use crate::cache::{CachedField, FieldCache};
use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// Fetch project metadata (project ID + field definitions)
// ---------------------------------------------------------------------------

const PROJECT_META_QUERY: &str = r"
query($org: String!, $number: Int!) {
  organization(login: $org) {
    projectV2(number: $number) {
      id
      fields(first: 50) {
        nodes {
          __typename
          ... on ProjectV2Field {
            id
            name
            dataType
          }
          ... on ProjectV2SingleSelectField {
            id
            name
            options {
              id
              name
            }
          }
        }
      }
    }
  }
}
";

#[derive(Debug, Deserialize)]
struct MetaResponse {
    data: Option<MetaData>,
    errors: Option<Vec<GqlError>>,
}

#[derive(Debug, Deserialize)]
struct MetaData {
    organization: Option<MetaOrg>,
}

#[derive(Debug, Deserialize)]
struct MetaOrg {
    #[serde(rename = "projectV2")]
    project_v2: Option<MetaProject>,
}

#[derive(Debug, Deserialize)]
struct MetaProject {
    id: String,
    fields: MetaFieldConnection,
}

#[derive(Debug, Deserialize)]
struct MetaFieldConnection {
    nodes: Vec<MetaFieldNode>,
}

#[derive(Debug, Deserialize)]
struct MetaFieldNode {
    #[serde(rename = "__typename")]
    typename: String,
    id: Option<String>,
    name: Option<String>,
    options: Option<Vec<MetaFieldOption>>,
}

#[derive(Debug, Deserialize)]
struct MetaFieldOption {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct GqlError {
    message: String,
}

pub fn fetch_field_cache(gh: &Gh, org: &str, number: u32) -> Result<FieldCache> {
    let query_arg = format!("query={PROJECT_META_QUERY}");
    let org_arg = format!("org={org}");
    let number_arg = format!("number={number}");

    let output = gh
        .run(&[
            "api",
            "graphql",
            "-f",
            &query_arg,
            "-F",
            &org_arg,
            "-F",
            &number_arg,
        ])
        .map_err(armitage_github::error::Error::from)?;

    let resp: MetaResponse = serde_json::from_str(&output)?;
    if let Some(errs) = resp.errors {
        let msg = errs
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(Error::Other(format!("GraphQL error: {msg}")));
    }

    let project = resp
        .data
        .and_then(|d| d.organization)
        .and_then(|o| o.project_v2)
        .ok_or_else(|| Error::Other(format!("project {org}/{number} not found")))?;

    let mut fields = HashMap::new();
    for node in project.fields.nodes {
        let Some(id) = node.id else { continue };
        let Some(name) = node.name else { continue };
        let cached = match node.typename.as_str() {
            "ProjectV2SingleSelectField" => {
                let options = node
                    .options
                    .unwrap_or_default()
                    .into_iter()
                    .map(|o| (o.name, o.id))
                    .collect();
                CachedField::SingleSelect { id, options }
            }
            _ => CachedField::Date { id },
        };
        fields.insert(name, cached);
    }

    Ok(FieldCache {
        project_id: project.id,
        cached_at: chrono::Utc::now().to_rfc3339(),
        fields,
    })
}

// ---------------------------------------------------------------------------
// Fetch issue node ID
// ---------------------------------------------------------------------------

const ISSUE_ID_QUERY: &str = r"
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    issue(number: $number) {
      id
    }
  }
}
";

#[derive(Debug, Deserialize)]
struct IssueIdResponse {
    data: Option<IssueIdData>,
    errors: Option<Vec<GqlError>>,
}

#[derive(Debug, Deserialize)]
struct IssueIdData {
    repository: Option<IssueIdRepo>,
}

#[derive(Debug, Deserialize)]
struct IssueIdRepo {
    issue: Option<IssueIdIssue>,
}

#[derive(Debug, Deserialize)]
struct IssueIdIssue {
    id: String,
}

pub fn fetch_issue_node_id(gh: &Gh, owner: &str, repo: &str, number: u64) -> Result<String> {
    let query_arg = format!("query={ISSUE_ID_QUERY}");
    let owner_arg = format!("owner={owner}");
    let name_arg = format!("name={repo}");
    let number_arg = format!("number={number}");

    let output = gh
        .run(&[
            "api",
            "graphql",
            "-f",
            &query_arg,
            "-f",
            &owner_arg,
            "-f",
            &name_arg,
            "-F",
            &number_arg,
        ])
        .map_err(armitage_github::error::Error::from)?;

    let resp: IssueIdResponse = serde_json::from_str(&output)?;
    if let Some(errs) = resp.errors {
        let msg = errs
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(Error::Other(format!("GraphQL error: {msg}")));
    }

    resp.data
        .and_then(|d| d.repository)
        .and_then(|r| r.issue)
        .map(|i| i.id)
        .ok_or_else(|| Error::Other(format!("{owner}/{repo}#{number} not found")))
}

// ---------------------------------------------------------------------------
// Add issue to project
// ---------------------------------------------------------------------------

const ADD_ITEM_MUTATION: &str = r"
mutation($projectId: ID!, $contentId: ID!) {
  addProjectV2ItemById(input: {projectId: $projectId, contentId: $contentId}) {
    item {
      id
    }
  }
}
";

#[derive(Debug, Deserialize)]
struct AddItemResponse {
    data: Option<AddItemData>,
    errors: Option<Vec<GqlError>>,
}

#[derive(Debug, Deserialize)]
struct AddItemData {
    #[serde(rename = "addProjectV2ItemById")]
    add_item: Option<AddItemPayload>,
}

#[derive(Debug, Deserialize)]
struct AddItemPayload {
    item: Option<AddedItem>,
}

#[derive(Debug, Deserialize)]
struct AddedItem {
    id: String,
}

pub fn add_item_to_project(gh: &Gh, project_id: &str, content_node_id: &str) -> Result<String> {
    let query_arg = format!("query={ADD_ITEM_MUTATION}");
    let project_arg = format!("projectId={project_id}");
    let content_arg = format!("contentId={content_node_id}");

    let output = gh
        .run(&[
            "api",
            "graphql",
            "-f",
            &query_arg,
            "-F",
            &project_arg,
            "-F",
            &content_arg,
        ])
        .map_err(armitage_github::error::Error::from)?;

    let resp: AddItemResponse = serde_json::from_str(&output)?;
    if let Some(errs) = resp.errors {
        let msg = errs
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(Error::Other(format!("GraphQL error: {msg}")));
    }

    resp.data
        .and_then(|d| d.add_item)
        .and_then(|p| p.item)
        .map(|i| i.id)
        .ok_or_else(|| Error::Other("addProjectV2ItemById returned no item".into()))
}

// ---------------------------------------------------------------------------
// Update a date field
// ---------------------------------------------------------------------------

const UPDATE_DATE_MUTATION: &str = r"
mutation($projectId: ID!, $itemId: ID!, $fieldId: ID!, $value: Date!) {
  updateProjectV2ItemFieldValue(input: {
    projectId: $projectId
    itemId: $itemId
    fieldId: $fieldId
    value: { date: $value }
  }) {
    projectV2Item { id }
  }
}
";

pub fn update_date_field(
    gh: &Gh,
    project_id: &str,
    item_id: &str,
    field_id: &str,
    date: &str,
) -> Result<()> {
    let query_arg = format!("query={UPDATE_DATE_MUTATION}");
    let project_arg = format!("projectId={project_id}");
    let item_arg = format!("itemId={item_id}");
    let field_arg = format!("fieldId={field_id}");
    let value_arg = format!("value={date}");

    let output = gh
        .run(&[
            "api",
            "graphql",
            "-f",
            &query_arg,
            "-F",
            &project_arg,
            "-F",
            &item_arg,
            "-F",
            &field_arg,
            "-f",
            &value_arg,
        ])
        .map_err(armitage_github::error::Error::from)?;

    check_for_errors(&output)
}

// ---------------------------------------------------------------------------
// Update a single-select field (status)
// ---------------------------------------------------------------------------

const UPDATE_SELECT_MUTATION: &str = r"
mutation($projectId: ID!, $itemId: ID!, $fieldId: ID!, $optionId: String!) {
  updateProjectV2ItemFieldValue(input: {
    projectId: $projectId
    itemId: $itemId
    fieldId: $fieldId
    value: { singleSelectOptionId: $optionId }
  }) {
    projectV2Item { id }
  }
}
";

pub fn update_single_select_field(
    gh: &Gh,
    project_id: &str,
    item_id: &str,
    field_id: &str,
    option_id: &str,
) -> Result<()> {
    let query_arg = format!("query={UPDATE_SELECT_MUTATION}");
    let project_arg = format!("projectId={project_id}");
    let item_arg = format!("itemId={item_id}");
    let field_arg = format!("fieldId={field_id}");
    let option_arg = format!("optionId={option_id}");

    let output = gh
        .run(&[
            "api",
            "graphql",
            "-f",
            &query_arg,
            "-F",
            &project_arg,
            "-F",
            &item_arg,
            "-F",
            &field_arg,
            "-f",
            &option_arg,
        ])
        .map_err(armitage_github::error::Error::from)?;

    check_for_errors(&output)
}

// ---------------------------------------------------------------------------
// Fetch existing project items (to detect no-ops)
// ---------------------------------------------------------------------------

const PROJECT_ITEMS_QUERY: &str = r"
query($org: String!, $number: Int!, $cursor: String) {
  organization(login: $org) {
    projectV2(number: $number) {
      items(first: 100, after: $cursor) {
        pageInfo { hasNextPage endCursor }
        nodes {
          id
          content {
            ... on Issue {
              number
              repository { nameWithOwner }
            }
          }
          fieldValues(first: 20) {
            nodes {
              ... on ProjectV2ItemFieldDateValue {
                date
                field { ... on ProjectV2Field { name } }
              }
              ... on ProjectV2ItemFieldSingleSelectValue {
                name
                field { ... on ProjectV2SingleSelectField { name } }
              }
            }
          }
        }
      }
    }
  }
}
";

/// Snapshot of a single item currently in the project board.
#[derive(Debug, Clone)]
pub struct ProjectItem {
    pub item_id: String,
    /// `owner/repo#number`
    pub issue_ref: String,
    /// Field name → current value (date string or option name).
    pub field_values: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ItemsResponse {
    data: Option<ItemsData>,
    errors: Option<Vec<GqlError>>,
}

#[derive(Debug, Deserialize)]
struct ItemsData {
    organization: Option<ItemsOrg>,
}

#[derive(Debug, Deserialize)]
struct ItemsOrg {
    #[serde(rename = "projectV2")]
    project_v2: Option<ItemsProject>,
}

#[derive(Debug, Deserialize)]
struct ItemsProject {
    items: ItemsConnection,
}

#[derive(Debug, Deserialize)]
struct ItemsConnection {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<ItemNode>,
}

#[derive(Debug, Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ItemNode {
    id: String,
    content: Option<ItemContent>,
    #[serde(rename = "fieldValues")]
    field_values: ItemFieldValues,
}

#[derive(Debug, Deserialize)]
struct ItemContent {
    number: Option<u64>,
    repository: Option<ItemRepo>,
}

#[derive(Debug, Deserialize)]
struct ItemRepo {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

#[derive(Debug, Deserialize)]
struct ItemFieldValues {
    nodes: Vec<ItemFieldValue>,
}

#[derive(Debug, Deserialize)]
struct ItemFieldValue {
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    field: Option<FieldRef>,
}

#[derive(Debug, Deserialize)]
struct FieldRef {
    #[serde(default)]
    name: Option<String>,
}

pub fn fetch_project_items(
    gh: &Gh,
    org: &str,
    number: u32,
) -> Result<HashMap<String, ProjectItem>> {
    let mut items: HashMap<String, ProjectItem> = HashMap::new();
    let mut cursor: Option<String> = None;

    loop {
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
        if let Some(ref c) = cursor {
            cursor_arg = format!("cursor={c}");
            args.push("-f");
            args.push(&cursor_arg);
        }

        let output = gh.run(&args).map_err(armitage_github::error::Error::from)?;

        let resp: ItemsResponse = serde_json::from_str(&output)?;
        if let Some(errs) = resp.errors {
            let msg = errs
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(Error::Other(format!("GraphQL error: {msg}")));
        }

        let connection = resp
            .data
            .and_then(|d| d.organization)
            .and_then(|o| o.project_v2)
            .map(|p| p.items)
            .ok_or_else(|| Error::Other(format!("project {org}/{number} not found")))?;

        for node in connection.nodes {
            let Some(content) = node.content else {
                continue;
            };
            let Some(number) = content.number else {
                continue;
            };
            let Some(repo) = content.repository else {
                continue;
            };
            let issue_ref = format!("{}#{}", repo.name_with_owner, number);

            let mut field_values = HashMap::new();
            for fv in node.field_values.nodes {
                let Some(field) = fv.field else { continue };
                let Some(field_name) = field.name else {
                    continue;
                };
                if let Some(date) = fv.date {
                    field_values.insert(field_name, date);
                } else if let Some(name) = fv.name {
                    field_values.insert(field_name, name);
                }
            }

            items.insert(
                issue_ref.clone(),
                ProjectItem {
                    item_id: node.id,
                    issue_ref,
                    field_values,
                },
            );
        }

        if !connection.page_info.has_next_page {
            break;
        }
        cursor = connection.page_info.end_cursor;
    }

    Ok(items)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ErrorOnlyResponse {
    errors: Option<Vec<GqlError>>,
}

fn check_for_errors(output: &str) -> Result<()> {
    let resp: ErrorOnlyResponse = serde_json::from_str(output)?;
    if let Some(errs) = resp.errors {
        let msg = errs
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(Error::Other(format!("GraphQL error: {msg}")));
    }
    Ok(())
}
