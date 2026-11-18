use super::{DemandEvidence, InvestigationSources, LinearReference, LinearWrite, ProductConfig};
use crate::Report;
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

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
        );",
    )?;
    Ok(())
}

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
pub(super) struct StoredInvestigation {
    pub result: InvestigationReport,
    writes: Vec<Option<LinearWrite>>,
}

impl StoredInvestigation {
    pub fn new(report: &Report, needs_identity_review: bool) -> Self {
        Self {
            result: InvestigationReport {
                product_id: report.product_id.clone(),
                report_id: report.id,
                needs_identity_review,
                findings: Vec::new(),
            },
            writes: Vec::new(),
        }
    }

    pub fn findings(&mut self, findings: Vec<(Finding, Option<LinearWrite>)>) {
        for (mut finding, write) in findings {
            finding.write = write.as_ref().map(|_| WriteStatus::Pending);
            finding.operation_id = write.as_ref().map(|write| write.operation_id().to_owned());
            self.result.findings.push(finding);
            self.writes.push(write);
        }
    }

    pub fn pending(&self) -> bool {
        self.result
            .findings
            .iter()
            .any(|finding| matches!(finding.write, Some(WriteStatus::Pending)))
    }

    pub fn deliver(
        &mut self,
        connection: &Connection,
        root_id: i64,
        product: &ProductConfig,
        sources: &mut impl InvestigationSources,
    ) -> Result<()> {
        let previous_unknown =
            has_previous_uncertain_write(connection, &product.product_id, root_id)?;
        for index in 0..self.writes.len() {
            let finding = &mut self.result.findings[index];
            if !matches!(finding.write, Some(WriteStatus::Pending)) {
                continue;
            }
            let write = self.writes[index]
                .as_ref()
                .context("saved write intent is missing")?;
            let repo = product
                .repositories
                .iter()
                .find(|route| finding.repository.as_deref() == Some(&route.repository));
            let allowed = match write {
                LinearWrite::Comment { ticket, .. } => {
                    if product.scopes.contains(&ticket.scope) && repo.is_some() {
                        let current = sources.ticket(&ticket.id)?;
                        current == **ticket
                            && !product.not_needed_state_ids.contains(&current.state_id)
                    } else {
                        false
                    }
                }
                LinearWrite::Create {
                    destination,
                    proposal_state_id,
                    ..
                } => repo.is_some_and(|route| {
                    &route.destination == destination
                        && &route.proposal_state_id == proposal_state_id
                }),
            };
            let unresolved = previous_unknown
                || self
                    .result
                    .findings
                    .iter()
                    .any(|item| matches!(item.write, Some(WriteStatus::Uncertain)));
            let finding = &mut self.result.findings[index];
            if !allowed || unresolved {
                finding.kind = FindingKind::HumanReview;
                finding.reason = if unresolved {
                    "A Linear write for this product needs reconciliation before more writes."
                        .into()
                } else {
                    "The Linear issue or configured proposal destination changed during investigation.".into()
                };
                finding.write = None;
                save(connection, root_id, self)?;
                continue;
            }
            sources.validate_write(write)?;
            // A process may stop after Linear accepts the write but before recording its response.
            // Commit this state before sending anything, and never retry it automatically.
            finding.write = Some(WriteStatus::Uncertain);
            save(connection, root_id, self)?;
            let status = match sources.write(write) {
                Ok(reference)
                    if reference.id == write.operation_id() && !reference.url.is_empty() =>
                {
                    WriteStatus::Completed { reference }
                }
                _ => WriteStatus::Uncertain,
            };
            self.result.findings[index].write = Some(status);
            save(connection, root_id, self)?;
        }
        Ok(())
    }
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
        stored.result.product_id == product_id
            && stored.writes.len() == stored.result.findings.len(),
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
                SELECT 1 FROM json_each(result, '$.result.findings')
                WHERE json_extract(value, '$.write.status') = 'uncertain'
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
        params![
            stored.result.product_id,
            root_id,
            serde_json::to_string(stored)?
        ],
    )?;
    Ok(())
}
