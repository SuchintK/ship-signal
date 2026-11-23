use super::Sources;
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, HeaderValue};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use ship_signal::investigation::{
    LinearReference, LinearScope, LinearWrite, ProductConfig, RejectionDecision, RejectionReason,
    Ticket,
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

impl PageInfo {
    fn next(self, seen: &mut BTreeSet<String>) -> Result<Option<String>> {
        if !self.has_next_page {
            return Ok(None);
        }
        let cursor = self
            .end_cursor
            .context("Linear omitted a pagination cursor")?;
        ensure!(
            seen.insert(cursor.clone()),
            "Linear repeated a pagination cursor"
        );
        Ok(Some(cursor))
    }
}

#[derive(Deserialize)]
struct IssueActivity<T> {
    issue: Activity<T>,
}

#[derive(Deserialize)]
struct Activity<T> {
    activity: Page<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusHistory {
    id: String,
    created_at: DateTime<Utc>,
    actor_id: Option<String>,
    from_state_id: Option<String>,
    to_state_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueComment {
    id: String,
    created_at: DateTime<Utc>,
    user: Option<Id>,
    body: String,
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

fn rejection_decision(
    ticket: &Ticket,
    rejection_states: &[String],
    history: Vec<StatusHistory>,
) -> Option<RejectionDecision> {
    if !rejection_states.contains(&ticket.state_id) {
        return None;
    }
    let mut expected_state = ticket.state_id.clone();
    for change in history {
        match (&change.from_state_id, &change.to_state_id) {
            (None, None) => continue,
            (Some(from), Some(to)) if *to == expected_state && from != to => {
                if !rejection_states.contains(from) {
                    return Some(RejectionDecision {
                        ticket_id: ticket.id.clone(),
                        history_id: change.id,
                        decided_at: change.created_at,
                        actor_id: change.actor_id,
                    });
                }
                expected_state = from.clone();
            }
            _ => return None,
        }
    }
    None
}

fn rejection_reason(
    decision: &RejectionDecision,
    comments: Vec<IssueComment>,
) -> Option<RejectionReason> {
    let actor_id = decision.actor_id.as_ref()?;
    let mut reason: Option<RejectionReason> = None;
    for comment in comments {
        if comment.created_at <= decision.decided_at
            || comment
                .user
                .as_ref()
                .is_none_or(|user| &user.id != actor_id)
        {
            continue;
        }
        let Some(text) = comment.body.trim_start().strip_prefix("Rejection reason:") else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() || reason.as_ref().is_some_and(|reason| reason.text != text) {
            return None;
        }
        if reason.is_none() {
            reason = Some(RejectionReason {
                comment_id: comment.id,
                text: text.into(),
            });
        }
    }
    reason
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

    /// Fetches Linear issues for the selected product's team and project scopes.
    /// A scope without a project fetches unprojected issues for that team.
    /// Reads all pages, including archived issues, and converts the results to `Ticket` records.
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
                    tickets.push(issue.into_ticket()?);
                }
                after = page.page_info.next(&mut seen)?;
                if after.is_none() {
                    break;
                }
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

    fn issue_activity<T: DeserializeOwned>(
        &self,
        issue_id: &str,
        field: &str,
        fields: &str,
    ) -> Result<Vec<T>> {
        let query = format!(
            "query InvestigationActivity($id: String!, $after: String) {{
                issue(id: $id) {{
                    activity: {field}(first: 50, after: $after, orderBy: createdAt) {{
                        nodes {{ {fields} }}
                        pageInfo {{ hasNextPage endCursor }}
                    }}
                }}
            }}"
        );
        let mut items = Vec::new();
        let mut after = None;
        let mut seen = BTreeSet::new();
        loop {
            let page = self
                .linear::<IssueActivity<T>>(
                    field,
                    &query,
                    json!({ "id": issue_id, "after": after }),
                )?
                .issue
                .activity;
            items.extend(page.nodes);
            after = page.page_info.next(&mut seen)?;
            if after.is_none() {
                return Ok(items);
            }
        }
    }

    pub(super) fn linear_rejection_decision(
        &self,
        ticket: &Ticket,
        rejection_states: &[String],
    ) -> Result<Option<RejectionDecision>> {
        let history = self.issue_activity(
            &ticket.id,
            "history",
            "id createdAt actorId fromStateId toStateId",
        )?;
        Ok(rejection_decision(ticket, rejection_states, history))
    }

    pub(super) fn linear_rejection_reason(
        &self,
        decision: &RejectionDecision,
    ) -> Result<Option<RejectionReason>> {
        if decision.actor_id.is_none() {
            return Ok(None);
        }
        let comments = self.issue_activity(
            &decision.ticket_id,
            "comments",
            "id body createdAt user { id }",
        )?;
        Ok(rejection_reason(decision, comments))
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
        result.confirmed_reference()
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

impl MutationResult {
    fn confirmed_reference(self) -> Result<LinearReference> {
        ensure!(
            self.success,
            "Linear did not confirm the investigation mutation"
        );
        self.reference
            .context("Linear returned no mutation reference")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_uses_the_latest_entry_into_the_rejected_states() -> Result<()> {
        let ticket = serde_json::from_value::<Ticket>(json!({
            "id": "email",
            "url": "https://linear.app/example/issue/EX-123",
            "scope": { "team_id": "team" },
            "title": "Email alert",
            "description": "Send email",
            "state_id": "archived-rejection",
            "state_name": "Won't do",
            "state_type": "canceled",
            "archived": true,
            "labels": [],
            "pull_request_urls": []
        }))?;
        let states = ["rejected".into(), "archived-rejection".into()];
        let history = |id, created_at, from, to| {
            serde_json::from_value::<StatusHistory>(json!({
                "id": id,
                "createdAt": created_at,
                "actorId": "reviewer",
                "fromStateId": from,
                "toStateId": to
            }))
        };
        let changes = vec![
            history("edit", "2026-09-03T10:00:00Z", None, None)?,
            history(
                "move",
                "2026-09-02T10:00:00Z",
                Some("rejected"),
                Some("archived-rejection"),
            )?,
            history(
                "reject",
                "2026-09-01T10:00:00Z",
                Some("planned"),
                Some("rejected"),
            )?,
            history(
                "reopen",
                "2026-08-31T10:00:00Z",
                Some("rejected"),
                Some("planned"),
            )?,
        ];
        let decision = rejection_decision(&ticket, &states, changes).unwrap();
        assert_eq!(decision.history_id, "reject");
        assert_eq!(
            decision.decided_at,
            "2026-09-01T10:00:00Z".parse::<DateTime<Utc>>()?
        );

        let changes = vec![history(
            "mismatch",
            "2026-09-02T10:00:00Z",
            Some("planned"),
            Some("rejected"),
        )?];
        assert!(rejection_decision(&ticket, &states, changes).is_none());
        Ok(())
    }

    #[test]
    fn reason_requires_a_marked_comment_from_the_decision_author() -> Result<()> {
        let decision = RejectionDecision {
            ticket_id: "email".into(),
            history_id: "reject".into(),
            decided_at: "2026-09-01T10:00:00Z".parse()?,
            actor_id: Some("reviewer".into()),
        };
        let comment = |id, created_at, user, body| {
            serde_json::from_value::<IssueComment>(json!({
                "id": id,
                "createdAt": created_at,
                "user": { "id": user },
                "body": body
            }))
        };
        let before = comment(
            "before",
            "2026-09-01T09:59:59Z",
            "reviewer",
            "Rejection reason: Old reason",
        )?;
        let wrong_user = comment(
            "other",
            "2026-09-01T10:00:01Z",
            "someone-else",
            "Rejection reason: Unrelated",
        )?;
        let correct = || {
            comment(
                "reason",
                "2026-09-01T10:00:01Z",
                "reviewer",
                "Rejection reason: Wait for inventory.",
            )
        };
        assert_eq!(
            rejection_reason(&decision, vec![before, wrong_user, correct()?])
                .unwrap()
                .text,
            "Wait for inventory."
        );
        let conflicting = comment(
            "conflict",
            "2026-09-01T10:00:02Z",
            "reviewer",
            "Rejection reason: Different reason",
        )?;
        assert!(rejection_reason(&decision, vec![correct()?, conflicting]).is_none());
        Ok(())
    }

    #[test]
    fn linear_proposal_and_comment_responses_keep_the_operation_reference() -> Result<()> {
        let id = "e8ebcc31-d05f-4c13-bafc-2bd980e0ca2d";
        let url = "https://linear.app/example/issue/EX-123";
        let issue: Envelope<IssueCreated> = serde_json::from_value(json!({
            "data": { "issueCreate": { "success": true, "issue": { "id": id, "url": url } } }
        }))?;
        let comment: Envelope<CommentCreated> = serde_json::from_value(json!({
            "data": { "commentCreate": { "success": true, "comment": { "id": id, "url": url } } }
        }))?;
        assert_eq!(
            issue.data.unwrap().issue_create.confirmed_reference()?.id,
            id
        );
        assert_eq!(
            comment
                .data
                .unwrap()
                .comment_create
                .confirmed_reference()?
                .id,
            id
        );
        Ok(())
    }

    #[test]
    fn a_failed_mutation_cannot_confirm_a_write() -> Result<()> {
        let response: Envelope<IssueCreated> = serde_json::from_value(json!({
            "data": { "issueCreate": { "success": false, "issue": null } }
        }))?;
        assert!(
            response
                .data
                .unwrap()
                .issue_create
                .confirmed_reference()
                .is_err()
        );
        Ok(())
    }
}
