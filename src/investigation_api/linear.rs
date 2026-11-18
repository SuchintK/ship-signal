use super::Sources;
use anyhow::{Context, Result, ensure};
use reqwest::header::{AUTHORIZATION, HeaderValue};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use ship_signal::investigation::{
    LinearReference, LinearScope, LinearWrite, ProductConfig, Ticket,
};
use std::collections::BTreeSet;

const ISSUE_FIELDS: &str = "
    id url title description archivedAt
    team { id }
    project { id }
    state { id name type }
";
const ISSUE_CONTEXT: &str = "
    labels(first: 50) { nodes { name } pageInfo { hasNextPage } }
    attachments(first: 50) { nodes { url } pageInfo { hasNextPage } }
";

#[derive(Deserialize)]
struct Envelope<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page<T> {
    nodes: Vec<T>,
    page_info: PageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Issue {
    id: String,
    url: String,
    title: String,
    description: Option<String>,
    archived_at: Option<String>,
    team: Id,
    project: Option<Id>,
    state: State,
    labels: Page<Label>,
    attachments: Page<Attachment>,
}

#[derive(Deserialize)]
struct State {
    id: String,
    name: String,
    #[serde(rename = "type")]
    state_type: String,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
struct Attachment {
    url: String,
}

#[derive(Deserialize)]
struct Issues {
    issues: Page<Issue>,
}

#[derive(Deserialize)]
struct SingleIssue {
    issue: Issue,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProposalState {
    workflow_state: StateDestination,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StateDestination {
    team: Id,
    #[serde(rename = "type")]
    state_type: String,
    archived_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueCreated {
    issue_create: MutationResult,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentCreated {
    comment_create: MutationResult,
}

#[derive(Deserialize)]
struct MutationResult {
    success: bool,
    #[serde(alias = "comment", alias = "issue")]
    reference: Option<LinearReference>,
}

impl Issue {
    fn into_ticket(self) -> Result<Ticket> {
        ensure!(
            !self.labels.page_info.has_next_page && !self.attachments.page_info.has_next_page,
            "Linear issue has more labels or attachments than the investigation can inspect"
        );
        let description = self.description.unwrap_or_default();
        let pull_request_urls = super::github::pull_request_links(
            self.attachments
                .nodes
                .into_iter()
                .map(|attachment| attachment.url),
            &description,
        );
        let mut labels: Vec<_> = self
            .labels
            .nodes
            .into_iter()
            .map(|label| label.name)
            .collect();
        labels.sort_unstable();
        Ok(Ticket {
            id: self.id,
            url: self.url,
            scope: LinearScope {
                team_id: self.team.id,
                project_id: self.project.map(|project| project.id),
            },
            title: self.title,
            description,
            state_id: self.state.id,
            state_name: self.state.name,
            state_type: self.state.state_type,
            archived: self.archived_at.is_some(),
            labels,
            pull_request_urls,
        })
    }
}

impl Sources {
    fn linear<T: DeserializeOwned>(
        &self,
        operation: &str,
        query: &str,
        variables: Value,
    ) -> Result<T> {
        let token = std::env::var("LINEAR_API_KEY")
            .context("LINEAR_API_KEY is required for investigation")?;
        let mut authorization =
            HeaderValue::from_str(&token).context("invalid Linear API key header")?;
        authorization.set_sensitive(true);
        let response = self
            .http
            .post("https://api.linear.app/graphql")
            .header(AUTHORIZATION, authorization)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .with_context(|| format!("request Linear {operation}"))?
            .error_for_status()
            .with_context(|| format!("Linear {operation} HTTP status"))?
            .json::<Envelope<T>>()
            .with_context(|| format!("decode Linear {operation} response"))?;
        ensure!(
            response.errors.is_empty(),
            "Linear {operation} returned GraphQL errors"
        );
        response
            .data
            .with_context(|| format!("Linear {operation} returned no data"))
    }

    pub(super) fn linear_tickets(&self, product: &ProductConfig) -> Result<Vec<Ticket>> {
        let query = format!(
            "query InvestigationIssues($filter: IssueFilter!, $after: String) {{
                issues(first: 50, after: $after, includeArchived: true, filter: $filter) {{
                    nodes {{ {ISSUE_FIELDS} {ISSUE_CONTEXT} }}
                    pageInfo {{ hasNextPage endCursor }}
                }}
            }}"
        );
        let mut tickets = Vec::new();
        for scope in &product.scopes {
            let project = match &scope.project_id {
                Some(id) => json!({ "id": { "eq": id } }),
                None => json!({ "null": true }),
            };
            let filter = json!({ "team": { "id": { "eq": scope.team_id } }, "project": project });
            let mut after = None;
            let mut seen = BTreeSet::new();
            loop {
                let page = self
                    .linear::<Issues>(
                        "issue search",
                        &query,
                        json!({
                            "after": after, "filter": filter,
                        }),
                    )?
                    .issues;
                for issue in page.nodes {
                    if issue.team.id == scope.team_id
                        && issue.project.as_ref().map(|project| &project.id)
                            == scope.project_id.as_ref()
                    {
                        tickets.push(issue.into_ticket()?);
                    }
                }
                if !page.page_info.has_next_page {
                    break;
                }
                let next = page
                    .page_info
                    .end_cursor
                    .context("Linear omitted an issue search cursor")?;
                ensure!(
                    seen.insert(next.clone()),
                    "Linear repeated an issue search cursor"
                );
                after = Some(next);
            }
        }
        Ok(tickets)
    }

    pub(super) fn linear_ticket(&self, id: &str) -> Result<Ticket> {
        let query = format!(
            "query InvestigationIssue($id: String!) {{ issue(id: $id) {{ {ISSUE_FIELDS} {ISSUE_CONTEXT} }} }}"
        );
        self.linear::<SingleIssue>("issue lookup", &query, json!({ "id": id }))?
            .issue
            .into_ticket()
    }

    pub(super) fn linear_write(&self, write: &LinearWrite) -> Result<LinearReference> {
        let result = match write {
            LinearWrite::Comment {
                operation_id,
                ticket,
                body,
            } => {
                self.linear::<CommentCreated>(
                    "evidence comment",
                    "mutation InvestigationComment($input: CommentCreateInput!) {
                        commentCreate(input: $input) { success comment { id url } }
                    }",
                    json!({ "input": { "id": operation_id, "issueId": ticket.id, "body": body } }),
                )?
                .comment_create
            }
            LinearWrite::Create {
                operation_id,
                destination,
                proposal_state_id,
                title,
                body,
            } => {
                self.linear::<IssueCreated>(
                    "proposal",
                    "mutation InvestigationProposal($input: IssueCreateInput!) {
                        issueCreate(input: $input) { success issue { id url } }
                    }",
                    json!({ "input": { "id": operation_id, "teamId": destination.team_id,
                        "projectId": destination.project_id, "stateId": proposal_state_id,
                        "title": title, "description": body } }),
                )?
                .issue_create
            }
        };
        ensure!(
            result.success,
            "Linear did not confirm the investigation mutation"
        );
        result
            .reference
            .context("Linear returned no mutation reference")
    }

    pub(super) fn linear_validate_write(&self, write: &LinearWrite) -> Result<()> {
        if let LinearWrite::Create {
            destination,
            proposal_state_id,
            ..
        } = write
        {
            let state = self
                .linear::<ProposalState>(
                    "proposal state lookup",
                    "query InvestigationProposalState($id: String!) {
                    workflowState(id: $id) { type archivedAt team { id } }
                }",
                    json!({ "id": proposal_state_id }),
                )?
                .workflow_state;
            ensure!(
                state.team.id == destination.team_id
                    && state.archived_at.is_none()
                    && matches!(
                        state.state_type.as_str(),
                        "backlog" | "unstarted" | "triage"
                    ),
                "proposal state must be an active intake state belonging to the destination team"
            );
        }
        Ok(())
    }
}
