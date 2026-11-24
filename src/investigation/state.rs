use super::{
    DemandEvidence, InvestigationSources, LinearReference, LinearWrite, ProductConfig,
    RejectionEvidence, Ticket, WeeklyEvidence, scoped_tickets,
};
use crate::Report;
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS investigation_runs (
            product_id TEXT NOT NULL,
            original_report_id INTEGER NOT NULL REFERENCES analysis_reports(id),
            result TEXT NOT NULL,
            PRIMARY KEY (product_id, original_report_id)
        );
        CREATE TABLE IF NOT EXISTS investigation_scopes (
            team_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            product_id TEXT NOT NULL,
            PRIMARY KEY (team_id, project_id)
        );
        CREATE TABLE IF NOT EXISTS investigation_issues (
            issue_id TEXT PRIMARY KEY NOT NULL,
            product_id TEXT NOT NULL
        );",
    )?;
    Ok(())
}

// investigation_scopes remembers which product owns each Linear team and project pair.
// For example, if shop claims Engineering → Checkout, later investigations for shop can still search it.
// If billing tries to claim that same pair using a different investigation.json but the same SQLite file, Ship Signal stops before searching Linear.
// That keeps two products from investigating the same set of issues.
// The config check catches overlaps within one JSON file. The table catches overlaps across runs and config files that use the same SQLite state.
pub(super) fn claim_scopes(connection: &Connection, product: &ProductConfig) -> Result<()> {
    let tx = connection.unchecked_transaction()?;
    for scope in &product.scopes {
        let project = scope.project_id.as_deref().unwrap_or_default();
        tx.execute(
            "INSERT INTO investigation_scopes (team_id, project_id, product_id)
             VALUES (?1, ?2, ?3) ON CONFLICT(team_id, project_id) DO NOTHING",
            params![scope.team_id, project, product.product_id],
        )?;
        let owner: String = tx.query_row(
            "SELECT product_id FROM investigation_scopes WHERE team_id = ?1 AND project_id = ?2",
            params![scope.team_id, project],
            |row| row.get(0),
        )?;
        ensure!(
            owner == product.product_id,
            "Linear scope is assigned to another product in this state file"
        );
    }
    tx.commit()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationReport {
    pub product_id: String,
    pub report_id: i64,
    pub needs_identity_review: bool,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub need: DemandEvidence,
    pub variants: Vec<String>,
    pub kind: FindingKind,
    pub reason: String,
    pub repository: Option<String>,
    pub tickets: Vec<String>,
    pub code: Vec<String>,
    pub write: Option<WriteStatus>,
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection: Option<RejectionEvidence>,
}

impl Finding {
    pub(super) fn review(need: DemandEvidence, reason: String) -> Self {
        Self {
            variants: need.variants.clone(),
            need,
            kind: FindingKind::HumanReview,
            reason,
            repository: None,
            tickets: Vec::new(),
            code: Vec::new(),
            write: None,
            operation_id: None,
            rejection: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    MatchingWork,
    PossibleBugOrDiscoverability,
    Proposal,
    HumanReview,
    RejectedWork,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WriteStatus {
    Pending,
    Completed { reference: LinearReference },
    Uncertain,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    finding: Finding,
    write: Option<LinearWrite>,
}

#[derive(Serialize, Deserialize)]
struct RejectionCatalog {
    state_ids: BTreeSet<String>,
    tickets: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct StoredInvestigation {
    product_id: String,
    pub report_id: i64,
    pub needs_identity_review: bool,
    ticket_catalog: BTreeMap<String, String>,
    #[serde(default)]
    rejection_catalog: Option<RejectionCatalog>,
    entries: Vec<Entry>,
}

impl StoredInvestigation {
    pub fn new(report: &Report, product: &ProductConfig) -> Self {
        Self {
            product_id: report.product_id.clone(),
            report_id: report.id,
            needs_identity_review: report.productReport.needs_identity_review,
            rejection_catalog: Some(RejectionCatalog {
                state_ids: product.not_needed_state_ids.iter().cloned().collect(),
                tickets: BTreeMap::new(),
            }),
            ticket_catalog: BTreeMap::new(),
            entries: Vec::new(),
        }
    }

    pub fn remember_tickets(&mut self, tickets: &[Ticket]) -> Result<()> {
        self.ticket_catalog = ticket_catalog(tickets.iter())?;
        if let Some(rejections) = &mut self.rejection_catalog {
            rejections.tickets = ticket_catalog(
                tickets
                    .iter()
                    .filter(|ticket| rejections.state_ids.contains(&ticket.state_id)),
            )?;
        }
        Ok(())
    }

    pub fn into_report(self) -> InvestigationReport {
        InvestigationReport {
            product_id: self.product_id,
            report_id: self.report_id,
            needs_identity_review: self.needs_identity_review,
            findings: self
                .entries
                .into_iter()
                .map(|entry| entry.finding)
                .collect(),
        }
    }

    pub fn findings(&mut self, findings: Vec<(Finding, Option<LinearWrite>)>) {
        for (mut finding, write) in findings {
            finding.write = write.as_ref().map(|_| WriteStatus::Pending);
            finding.operation_id = write.as_ref().map(|write| write.operation_id().to_owned());
            self.entries.push(Entry { finding, write });
        }
    }

    pub fn pending(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.finding.write, Some(WriteStatus::Pending)))
    }

    pub fn deliver(
        &mut self,
        connection: &Connection,
        root_id: i64,
        product: &ProductConfig,
        sources: &mut impl InvestigationSources,
        weekly: WeeklyEvidence,
    ) -> Result<()> {
        let mut unresolved =
            has_previous_uncertain_write(connection, &product.product_id, root_id)?
                || self
                    .entries
                    .iter()
                    .any(|entry| matches!(entry.finding.write, Some(WriteStatus::Uncertain)));
        for index in 0..self.entries.len() {
            let entry = &self.entries[index];
            if !matches!(entry.finding.write, Some(WriteStatus::Pending)) {
                continue;
            }
            let write = entry
                .write
                .as_ref()
                .context("saved write intent is missing")?;
            let review = if unresolved {
                Some("A Linear write for this product needs reconciliation before more writes.")
            } else {
                self.review_before_write(entry, write, product, sources, weekly)?
            };
            if let Some(reason) = review {
                let finding = &mut self.entries[index].finding;
                finding.kind = FindingKind::HumanReview;
                finding.reason = reason.into();
                finding.write = None;
                save(connection, root_id, self)?;
                continue;
            }

            sources.validate_write(write)?;
            // Commit ownership and intent before sending anything to Linear.
            let tx = connection.unchecked_transaction()?;
            claim_issue(&tx, &product.product_id, write)?;
            self.entries[index].finding.write = Some(WriteStatus::Uncertain);
            save(&tx, root_id, self)?;
            tx.commit()?;

            let write = self.entries[index]
                .write
                .as_ref()
                .context("saved write intent is missing")?;
            let status = match sources.write(write) {
                Ok(reference)
                    if reference.id == write.operation_id() && !reference.url.is_empty() =>
                {
                    WriteStatus::Completed { reference }
                }
                _ => {
                    unresolved = true;
                    WriteStatus::Uncertain
                }
            };
            self.entries[index].finding.write = Some(status);
            save(connection, root_id, self)?;
        }
        Ok(())
    }

    fn review_before_write(
        &self,
        entry: &Entry,
        write: &LinearWrite,
        product: &ProductConfig,
        sources: &mut impl InvestigationSources,
        weekly: WeeklyEvidence,
    ) -> Result<Option<&'static str>> {
        let configured_states = product
            .not_needed_state_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let same_policy = self
            .rejection_catalog
            .as_ref()
            .map_or(configured_states.is_empty(), |saved| {
                saved.state_ids == configured_states
            });
        if !same_policy {
            return Ok(Some(
                "Review this write against the current Not needed configuration.",
            ));
        }
        let Some(route) = product
            .repositories
            .iter()
            .find(|route| entry.finding.repository.as_deref() == Some(&route.repository))
        else {
            return Ok(Some(
                "The configured repository changed during investigation.",
            ));
        };
        let changed = Some(
            "Linear work or the configured proposal destination changed during investigation.",
        );
        match write {
            LinearWrite::Comment { ticket, .. } => {
                if !product.scopes.contains(&ticket.scope) {
                    return Ok(changed);
                }
                let current = sources.ticket(&ticket.id)?;
                if current != **ticket || configured_states.contains(&current.state_id) {
                    return Ok(changed);
                }
            }
            LinearWrite::Create {
                destination,
                proposal_state_id,
                ..
            } => {
                if &route.destination != destination
                    || &route.proposal_state_id != proposal_state_id
                {
                    return Ok(changed);
                }
            }
        }

        if !configured_states.is_empty() || matches!(write, LinearWrite::Create { .. }) {
            let current = scoped_tickets(product, sources)?;
            let unchanged = match write {
                LinearWrite::Create { .. } => {
                    let catalog =
                        ticket_catalog(current.iter().filter(|ticket| !self.created_here(ticket)))?;
                    catalog == self.ticket_catalog
                }
                LinearWrite::Comment { .. } => {
                    let catalog = ticket_catalog(
                        current
                            .iter()
                            .filter(|ticket| configured_states.contains(&ticket.state_id)),
                    )?;
                    self.rejection_catalog
                        .as_ref()
                        .is_some_and(|saved| catalog == saved.tickets)
                }
            };
            if !unchanged {
                return Ok(changed);
            }
            if let Some(evidence) = &entry.finding.rejection {
                return recheck_rejection(evidence, &current, product, sources, weekly);
            }
        }
        Ok(None)
    }

    fn created_here(&self, ticket: &Ticket) -> bool {
        self.entries.iter().any(|entry| {
            if let (
                Some(LinearWrite::Create {
                    operation_id,
                    title,
                    body,
                    destination,
                    proposal_state_id,
                }),
                Some(WriteStatus::Completed { .. }),
            ) = (&entry.write, &entry.finding.write)
            {
                ticket.id == *operation_id
                    && ticket.title == *title
                    && ticket.description == *body
                    && ticket.scope == *destination
                    && ticket.state_id == *proposal_state_id
                    && !ticket.archived
            } else {
                false
            }
        })
    }
}

fn recheck_rejection(
    evidence: &RejectionEvidence,
    tickets: &[Ticket],
    product: &ProductConfig,
    sources: &mut impl InvestigationSources,
    weekly: WeeklyEvidence,
) -> Result<Option<&'static str>> {
    let changed = Some("The prior rejection changed during investigation.");
    let Some(latest) = evidence.decisions.first() else {
        return Ok(changed);
    };
    for decision in &evidence.decisions {
        let Some(ticket) = tickets.iter().find(|ticket| {
            ticket.id == decision.ticket_id
                && product.not_needed_state_ids.contains(&ticket.state_id)
        }) else {
            return Ok(changed);
        };
        if sources
            .rejection_decision(ticket, &product.not_needed_state_ids)?
            .as_ref()
            != Some(decision)
        {
            return Ok(changed);
        }
    }
    if let Some(reason) = &evidence.reason
        && sources.rejection_reason(latest)?.as_ref() != Some(reason)
    {
        return Ok(changed);
    }
    let now = sources.now();
    if !evidence.decisions.iter().all(|decision| {
        decision
            .expiry(now)
            .is_some_and(|expiry| weekly.qualifies_at(now, expiry))
    }) {
        return Ok(Some(
            "The weekly evidence is no longer current for reconsideration.",
        ));
    }
    Ok(None)
}

fn ticket_catalog<'a>(
    tickets: impl Iterator<Item = &'a Ticket>,
) -> Result<BTreeMap<String, String>> {
    tickets
        .map(|ticket| {
            let fingerprint = Sha256::digest(serde_json::to_vec(ticket)?);
            Ok((ticket.id.clone(), format!("{fingerprint:x}")))
        })
        .collect()
}

pub(super) fn foreign_issue(
    connection: &Connection,
    product_id: &str,
    issue_id: &str,
) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS (
            SELECT 1 FROM investigation_issues WHERE issue_id = ?1 AND product_id <> ?2
        )",
        params![issue_id, product_id],
        |row| row.get(0),
    )?)
}

fn claim_issue(connection: &Connection, product_id: &str, write: &LinearWrite) -> Result<()> {
    let id = match write {
        LinearWrite::Comment { ticket, .. } => &ticket.id,
        LinearWrite::Create { operation_id, .. } => operation_id,
    };
    connection.execute(
        "INSERT INTO investigation_issues (issue_id, product_id) VALUES (?1, ?2)
         ON CONFLICT(issue_id) DO NOTHING",
        params![id, product_id],
    )?;
    ensure!(
        !foreign_issue(connection, product_id, id)?,
        "Linear issue already belongs to another product"
    );
    Ok(())
}

pub(super) fn load(
    connection: &Connection,
    product_id: &str,
    root_id: i64,
) -> Result<Option<StoredInvestigation>> {
    let json: Option<String> = connection.query_row(
        "SELECT result FROM investigation_runs WHERE product_id = ?1 AND original_report_id = ?2",
        params![product_id, root_id],
        |row| row.get(0),
    ).optional()?;
    json.map(|json| decode(product_id, &json)).transpose()
}

fn decode(product_id: &str, json: &str) -> Result<StoredInvestigation> {
    let stored: StoredInvestigation = serde_json::from_str(json)?;
    ensure!(
        stored.product_id == product_id,
        "invalid saved investigation for the requested product"
    );
    Ok(stored)
}

fn has_previous_uncertain_write(
    connection: &Connection,
    product_id: &str,
    root_id: i64,
) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS (
            SELECT 1 FROM investigation_runs
            WHERE product_id = ?1 AND original_report_id <> ?2
            AND EXISTS (
                SELECT 1 FROM json_each(result, '$.entries')
                WHERE json_extract(value, '$.finding.write.status') = 'uncertain'
            )
         )",
        params![product_id, root_id],
        |row| row.get(0),
    )?)
}

pub(super) fn save(
    connection: &Connection,
    root_id: i64,
    stored: &StoredInvestigation,
) -> Result<()> {
    connection.execute(
        "INSERT INTO investigation_runs (product_id, original_report_id, result)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(product_id, original_report_id) DO UPDATE SET result = excluded.result",
        params![stored.product_id, root_id, serde_json::to_string(stored)?],
    )?;
    Ok(())
}
