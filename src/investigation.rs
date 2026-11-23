use crate::{NeedReport, Report, Workflow, validate_product_id};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs::OpenOptions};

mod rejection;
mod state;

use rejection::{Outcome, WeeklyEvidence};

const MAX_EVIDENCE_BYTES: usize = 200_000;

pub use rejection::{
    RejectionCoverage, RejectionDecision, RejectionEvidence, RejectionMatch, RejectionReason,
};
pub use state::{Finding, FindingKind, InvestigationReport, WriteStatus};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationConfig {
    pub products: Vec<ProductConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfig {
    pub product_id: String,
    pub scopes: Vec<LinearScope>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub not_needed_state_ids: Vec<String>,
    pub repositories: Vec<RepositoryRoute>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct LinearScope {
    pub team_id: String,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryRoute {
    pub repository: String,
    pub base_ref: String,
    pub live_ref: Option<String>,
    #[serde(default)]
    pub search_paths: Vec<String>,
    pub destination: LinearScope,
    pub proposal_state_id: String,
}

impl InvestigationConfig {
    /// Validates the full configuration and returns the requested product's settings by reference.
    ///
    /// Validation checks all products because each Linear team and project pair must have
    /// a single owner. It also checks that each repository sends proposals to a Linear scope
    /// configured for its product.
    ///
    /// Returns an error if the configuration is invalid or the requested product is missing.
    pub fn product(&self, product_id: &str) -> Result<&ProductConfig> {
        let mut scopes = BTreeSet::new();
        let mut products = BTreeSet::new();
        for product in &self.products {
            validate_product_id(&product.product_id)?;
            ensure!(
                products.insert(&product.product_id),
                "duplicate investigation product"
            );
            ensure!(!product.scopes.is_empty(), "product has no Linear scopes");
            for scope in &product.scopes {
                ensure!(
                    !scope.team_id.trim().is_empty()
                        && scope
                            .project_id
                            .as_ref()
                            .is_none_or(|id| !id.trim().is_empty()),
                    "Linear team and project IDs must be nonblank"
                );
                ensure!(
                    scopes.insert(scope),
                    "a Linear team and project scope must belong to exactly one product"
                );
            }
            let mut repositories = BTreeSet::new();
            ensure!(
                !product.repositories.is_empty(),
                "product has no repository routes"
            );
            for route in &product.repositories {
                ensure!(
                    valid_repository(&route.repository)
                        && repositories.insert(route.repository.to_ascii_lowercase()),
                    "invalid or duplicate GitHub repository route"
                );
                ensure!(
                    !route.base_ref.trim().is_empty()
                        && route
                            .live_ref
                            .as_ref()
                            .is_none_or(|name| !name.trim().is_empty())
                        && !route.proposal_state_id.trim().is_empty()
                        && !product
                            .not_needed_state_ids
                            .contains(&route.proposal_state_id),
                    "repository route requires a base ref and a proposal state"
                );
                ensure!(
                    product.scopes.contains(&route.destination),
                    "repository proposal destination must be inside its product's Linear scopes"
                );
            }
        }
        self.products
            .iter()
            .find(|product| product.product_id == product_id)
            .context("no investigation configuration for the requested product")
    }
}

pub fn valid_repository(repository: &str) -> bool {
    repository.split_once('/').is_some_and(|(owner, name)| {
        [owner, name].iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && part
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || b"-_.".contains(&ch))
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub id: String,
    pub url: String,
    pub scope: LinearScope,
    pub title: String,
    pub description: String,
    pub state_id: String,
    pub state_name: String,
    pub state_type: String,
    pub archived: bool,
    pub labels: Vec<String>,
    pub pull_request_urls: Vec<String>,
}

impl Ticket {
    fn planned_or_ongoing(&self) -> bool {
        !self.archived
            && matches!(
                self.state_type.as_str(),
                "backlog" | "unstarted" | "started"
            )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PullRequestEvidence {
    pub url: String,
    pub repository: String,
    pub title: String,
    pub description: String,
    pub state: String,
    pub merged: bool,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepositorySnapshot {
    pub repository: String,
    pub base_sha: String,
    pub live_sha: Option<String>,
    pub base_paths: Vec<String>,
    pub live_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CodeSelection {
    pub repository: String,
    pub path: String,
    pub live: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodeEvidence {
    pub repository: String,
    pub path: String,
    pub sha: String,
    pub live: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandEvidence {
    pub need: String,
    pub need_id: String,
    pub customer_count: usize,
    pub denominator: usize,
    pub percentage: f64,
    pub variants: Vec<String>,
    pub message_indices: Vec<usize>,
}

#[derive(Debug, Serialize)]
pub struct SearchContext<'a> {
    pub product_id: &'a str,
    pub demand: &'a DemandEvidence,
    pub labels: &'a [String],
    pub not_needed_state_ids: &'a [String],
    pub tickets: &'a [Ticket],
    pub pull_requests: &'a [PullRequestEvidence],
    pub repositories: &'a [RepositorySnapshot],
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentKind {
    MatchingWork,
    ExistingFunctionality,
    DistinctWork,
    Uncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Proposal {
    pub title: String,
    pub behavior: String,
    pub assumptions: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub scope_boundaries: Vec<String>,
    pub uses_existing_infrastructure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Assessment {
    pub kind: AssessmentKind,
    /// Zero-based slots. A need without explicit variants has one slot.
    pub variant_indices: Vec<usize>,
    pub repository: Option<String>,
    pub matching_ticket_ids: Vec<String>,
    pub related_ticket_ids: Vec<String>,
    pub rejection_matches: Vec<RejectionMatch>,
    pub code_indices: Vec<usize>,
    pub reason: String,
    pub proposal: Option<Proposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinearWrite {
    Comment {
        operation_id: String,
        ticket: Box<Ticket>,
        body: String,
    },
    Create {
        operation_id: String,
        destination: LinearScope,
        proposal_state_id: String,
        title: String,
        body: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearReference {
    pub id: String,
    pub url: String,
}

impl LinearWrite {
    fn operation_id(&self) -> &str {
        match self {
            Self::Comment { operation_id, .. } | Self::Create { operation_id, .. } => operation_id,
        }
    }
}

pub trait InvestigationSources {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
    fn rejection_decision(
        &mut self,
        _: &Ticket,
        _: &[String],
    ) -> Result<Option<RejectionDecision>> {
        Ok(None)
    }
    fn rejection_reason(&mut self, _: &RejectionDecision) -> Result<Option<RejectionReason>> {
        Ok(None)
    }
    fn tickets(&mut self, product: &ProductConfig) -> Result<Vec<Ticket>>;
    fn ticket(&mut self, id: &str) -> Result<Ticket>;
    fn pull_requests(
        &mut self,
        product: &ProductConfig,
        tickets: &[Ticket],
    ) -> Result<Vec<PullRequestEvidence>>;
    fn repository(&mut self, route: &RepositoryRoute) -> Result<RepositorySnapshot>;
    fn code(
        &mut self,
        snapshot: &RepositorySnapshot,
        selection: &CodeSelection,
    ) -> Result<CodeEvidence>;
    fn validate_write(&mut self, _: &LinearWrite) -> Result<()> {
        Ok(())
    }
    fn write(&mut self, write: &LinearWrite) -> Result<LinearReference>;
}

pub trait InvestigationModel {
    fn select_code(&mut self, context: &SearchContext<'_>) -> Result<Vec<CodeSelection>>;
    fn assess(
        &mut self,
        context: &SearchContext<'_>,
        code: &[CodeEvidence],
    ) -> Result<Vec<Assessment>>;
}

pub(super) fn initialize(connection: &Connection) -> Result<()> {
    state::initialize(connection)
}

impl Workflow {
    /// Investigate a saved report using its product's configured Linear and GitHub scope.
    /// After investigation, our report turns into an investigation.
    pub fn investigate(
        &self,
        product_id: &str,
        report_id: i64,
        config: &InvestigationConfig,
        sources: &mut impl InvestigationSources,
        model: &mut impl InvestigationModel,
    ) -> Result<InvestigationReport> {
        let report = self.report(product_id, report_id)?;
        let product = config.product(product_id)?;
        // Find the original report ID in the review chain
        let root_id = self.investigation_root(&report)?;

        // Lock the state file to prevent concurrent investigations
        let lock_path = self
            .connection
            .path()
            .context("investigation needs a local state file")?;
        ensure!(
            !lock_path.is_empty(),
            "investigation needs a local state file"
        );
        let path = std::fs::canonicalize(lock_path)?;
        let mut lock_path = path.into_os_string();
        lock_path.push(".investigation.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .context("open investigation lock")?;
        lock.try_lock().context("acquire investigation lock")?;

        if !report.product.needs_identity_review {
            state::claim_scopes(&self.connection, product)?;
        }
        let weekly = WeeklyEvidence::load(&self.connection, &report, root_id)?;
        let mut stored = match state::load(&self.connection, product_id, root_id)? {
            Some(stored) if !stored.needs_identity_review => {
                // Either the current report is from the first review or it's a later review of the same report.
                ensure!(
                    report_id == root_id || report_id == stored.report_id,
                    "this analysis already has an investigation for report {}",
                    stored.report_id
                );
                stored
            }
            _ => {
                let stored = plan(&self.connection, &report, product, sources, model, weekly)?;
                state::save(&self.connection, root_id, &stored)?;
                stored
            }
        };
        stored.deliver(&self.connection, root_id, product, sources, weekly)?;
        Ok(stored.into_report())
    }

    // investigation_root finds the original report ID in that review chain.
    //It starts with the report you supplied and follows reviewed_from until it reaches a report with no parent.
    // For example, in 45 → 42 → 41, it returns 41.
    // Ship Signal uses that ID so the original report and its reviewed versions share one investigation record.
    fn investigation_root(&self, report: &Report) -> Result<i64> {
        let mut id = report.id;
        // `review_report` records its input report here. Original analyses have no parent.
        let mut parent = report.reviewed_from;
        while let Some(parent_id) = parent {
            ensure!(parent_id < id, "invalid reviewed report lineage");
            let previous = self.report(&report.product_id, parent_id)?;
            ensure!(
                previous.start == report.start && previous.end == report.end,
                "reviewed report interval differs from its original"
            );
            id = previous.id;
            // Keep following parent links until we reach the original report.
            parent = previous.reviewed_from;
        }
        Ok(id)
    }

    pub(super) fn investigation_pending(&self, product_id: &str, root_id: i64) -> Result<bool> {
        Ok(state::load(&self.connection, product_id, root_id)?
            .is_none_or(|stored| stored.pending()))
    }
}

// product: the object that we obtained from the investigation
fn plan(
    connection: &Connection,
    report: &Report,
    product: &ProductConfig,
    sources: &mut impl InvestigationSources,
    model: &mut impl InvestigationModel,
    weekly: WeeklyEvidence,
) -> Result<state::StoredInvestigation> {
    let analysis = &report.product;
    let mut stored = state::StoredInvestigation::new(report, product);
    if analysis.needs_identity_review {
        return Ok(stored);
    }
    let demand = analysis
        .needs
        .iter()
        .filter(|need| need.qualifies)
        .map(|need| demand_evidence(report, need))
        .collect::<Result<Vec<_>>>()?;
    if demand.is_empty() {
        return Ok(stored);
    }

    // Load tickets, repository and PRs for checking the need
    let tickets = scoped_tickets(product, sources)?;
    for ticket in &tickets {
        if state::foreign_issue(connection, &product.product_id, &ticket.id)? {
            for evidence in demand {
                stored.findings(vec![(Finding::review(
                    evidence,
                    "A Linear issue in this scope previously received evidence for another product. Review its product ownership.".into(),
                ), None)]);
            }
            return Ok(stored);
        }
    }
    stored.remember_tickets(&tickets)?;
    let pull_requests = sources.pull_requests(product, &tickets)?;
    let repositories = product
        .repositories
        .iter()
        .map(|route| sources.repository(route))
        .collect::<Result<Vec<_>>>()?;

    for evidence in demand {
        let context = SearchContext {
            product_id: &product.product_id,
            demand: &evidence,
            labels: &product.labels,
            not_needed_state_ids: &product.not_needed_state_ids,
            tickets: &tickets,
            pull_requests: &pull_requests,
            repositories: &repositories,
        };
        stored.findings(investigate_need(
            report, &context, product, sources, model, weekly,
        )?);
    }
    Ok(stored)
}

fn scoped_tickets(
    product: &ProductConfig,
    sources: &mut impl InvestigationSources,
) -> Result<Vec<Ticket>> {
    let mut tickets = sources.tickets(product)?;
    tickets.retain(|ticket| product.scopes.contains(&ticket.scope));
    tickets.sort_by(|left, right| left.id.cmp(&right.id));
    tickets.dedup_by(|left, right| left.id == right.id);
    Ok(tickets)
}

fn demand_evidence(report: &Report, need: &NeedReport) -> Result<DemandEvidence> {
    ensure!(
        need.message_indices
            .iter()
            .all(|index| report.input.feedback.get(*index).is_some()),
        "saved report references a missing supporting message"
    );
    Ok(DemandEvidence {
        need: need.need.clone(),
        need_id: need
            .identity
            .clone()
            .context("eligible need has no reviewed identity")?,
        customer_count: need.customer_count,
        denominator: report.product.denominator,
        percentage: need.percentage,
        variants: need.variants.clone(),
        message_indices: need.message_indices.clone(),
    })
}

fn investigate_need(
    report: &Report,
    context: &SearchContext<'_>,
    product: &ProductConfig,
    sources: &mut impl InvestigationSources,
    model: &mut impl InvestigationModel,
    weekly: WeeklyEvidence,
) -> Result<Vec<(Finding, Option<LinearWrite>)>> {
    let review = |reason: String| {
        Ok(vec![(
            Finding::review(context.demand.clone(), reason),
            None,
        )])
    };
    if serde_json::to_vec(context)?.len() > MAX_EVIDENCE_BYTES {
        return review(
            "The product's ticket and repository evidence exceeds the investigation limit.".into(),
        );
    }
    let selections = model.select_code(context)?;
    let code = match read_selected_code(context, sources, selections) {
        Ok(code) => code,
        Err(error) => return review(error.to_string()),
    };
    if serde_json::to_vec(&(context, &code))?.len() > MAX_EVIDENCE_BYTES {
        return review("The selected code exceeds the investigation limit.".into());
    }
    let decisions = model.assess(context, &code)?;
    let count = context.demand.variants.len().max(1);
    let mut coverage: Vec<_> = decisions
        .iter()
        .flat_map(|decision| decision.variant_indices.iter().copied())
        .collect();
    coverage.sort_unstable();
    if !coverage.into_iter().eq(0..count)
        || decisions
            .iter()
            .any(|decision| decision.variant_indices.is_empty())
    {
        return review("Investigation did not cover each requested behavior exactly once.".into());
    }
    let mut findings = Vec::new();
    for decision in decisions {
        let variants = decision
            .variant_indices
            .iter()
            .filter_map(|&index| context.demand.variants.get(index).cloned())
            .collect();
        let outcome = rejection::evaluate(&decision, context, sources, weekly)?;
        let finding =
            classify_finding(report, context, product, &code, decision, variants, outcome)
                .unwrap_or_else(|error| {
                    (
                        Finding::review(context.demand.clone(), error.to_string()),
                        None,
                    )
                });
        findings.push(finding);
    }
    Ok(findings)
}

fn read_selected_code(
    context: &SearchContext<'_>,
    sources: &mut impl InvestigationSources,
    selections: Vec<CodeSelection>,
) -> Result<Vec<CodeEvidence>> {
    ensure!(
        selections.len() <= 12,
        "too many code selections to investigate reliably"
    );
    let mut code = Vec::new();
    for selection in selections {
        let repository = context
            .repositories
            .iter()
            .find(|repository| repository.repository == selection.repository)
            .context("investigation selected a repository outside the product")?;
        let (sha, paths) = if selection.live {
            (
                repository
                    .live_sha
                    .as_deref()
                    .context("investigation selected an unconfigured live ref")?,
                &repository.live_paths,
            )
        } else {
            (repository.base_sha.as_str(), &repository.base_paths)
        };
        ensure!(
            paths.contains(&selection.path),
            "investigation selected a path absent at the requested revision"
        );
        let retrieved = sources.code(repository, &selection)?;
        ensure!(
            retrieved.sha == sha
                && retrieved.repository == selection.repository
                && retrieved.path == selection.path
                && retrieved.live == selection.live,
            "retrieved code does not match the pinned repository and ref"
        );
        code.push(retrieved);
    }
    Ok(code)
}

fn classify_finding(
    report: &Report,
    context: &SearchContext<'_>,
    product: &ProductConfig,
    code: &[CodeEvidence],
    decision: Assessment,
    variants: Vec<String>,
    rejection: Outcome,
) -> Result<(Finding, Option<LinearWrite>)> {
    ensure!(
        !decision.reason.trim().is_empty(),
        "investigation did not explain its finding"
    );
    let ticket = |id: &str| {
        context
            .tickets
            .iter()
            .find(|ticket| ticket.id == id)
            .context("investigation cited an issue outside the product's search results")
    };
    let matching = decision
        .matching_ticket_ids
        .iter()
        .map(|id| ticket(id))
        .collect::<Result<Vec<_>>>()?;
    let related = decision
        .related_ticket_ids
        .iter()
        .map(|id| ticket(id))
        .collect::<Result<Vec<_>>>()?;
    let rejected = decision
        .rejection_matches
        .iter()
        .map(|item| ticket(&item.ticket_id))
        .collect::<Result<Vec<_>>>()?;
    let cited_code = decision
        .code_indices
        .iter()
        .map(|&index| code.get(index).context("investigation cited unread code"))
        .collect::<Result<Vec<_>>>()?;
    let route = decision
        .repository
        .as_ref()
        .map(|repository| {
            product
                .repositories
                .iter()
                .find(|route| route.repository == *repository)
                .context("investigation selected an unconfigured repository")
        })
        .transpose()?;
    let mut seen_urls = BTreeSet::new();
    let mut finding = Finding {
        need: context.demand.clone(),
        variants,
        kind: FindingKind::HumanReview,
        reason: decision.reason,
        repository: route.map(|route| route.repository.clone()),
        tickets: matching
            .iter()
            .chain(&related)
            .chain(&rejected)
            .filter(|ticket| seen_urls.insert(&ticket.url))
            .map(|ticket| ticket.url.clone())
            .collect(),
        code: cited_code.iter().map(|code| code_url(code)).collect(),
        write: None,
        operation_id: None,
        rejection: None,
    };
    match rejection {
        Outcome::Proceed(evidence) => finding.rejection = evidence,
        Outcome::Suppressed(reason) => {
            finding.kind = FindingKind::RejectedWork;
            finding.reason = reason;
            return Ok((finding, None));
        }
        Outcome::Review(reason) => {
            finding.reason = reason;
            return Ok((finding, None));
        }
    }
    if matches!(decision.kind, AssessmentKind::Uncertain) {
        return Ok((finding, None));
    }
    let Some(route) = route.filter(|route| {
        !cited_code.is_empty()
            && cited_code
                .iter()
                .all(|item| item.repository == route.repository && !item.text.trim().is_empty())
    }) else {
        finding.reason = format!("Repository ownership needs review. {}", finding.reason);
        return Ok((finding, None));
    };
    if matching
        .iter()
        .chain(&related)
        .chain(&rejected)
        .any(|ticket| {
            ticket.pull_request_urls.iter().any(|url| {
                !context
                    .pull_requests
                    .iter()
                    .any(|pull_request| pull_request.url == *url)
            })
        })
    {
        finding.reason = format!(
            "A linked pull request could not be inspected within the product scope. {}",
            finding.reason
        );
        return Ok((finding, None));
    }
    let write = match decision.kind {
        AssessmentKind::ExistingFunctionality if cited_code.iter().any(|code| code.live) => {
            finding.kind = FindingKind::PossibleBugOrDiscoverability;
            None
        }
        AssessmentKind::MatchingWork if matching.len() == 1 && matching[0].planned_or_ongoing() => {
            finding.kind = FindingKind::MatchingWork;
            Some(LinearWrite::Comment {
                operation_id: uuid::Uuid::new_v4().to_string(),
                ticket: Box::new(matching[0].clone()),
                body: evidence_markdown(report, &finding),
            })
        }
        AssessmentKind::DistinctWork if matching.is_empty() => {
            let Some(proposal) = decision.proposal.filter(bounded_proposal) else {
                finding.reason = format!("The proposed scope needs review. {}", finding.reason);
                return Ok((finding, None));
            };
            finding.kind = FindingKind::Proposal;
            let mut body = format!(
                "## Proposed behavior\n\n{}\n\n## Assumptions\n\n{}\n\n## Acceptance criteria\n\n{}\n\n## Scope boundaries\n\n{}\n\n{}",
                proposal.behavior,
                bullets(&proposal.assumptions, "- "),
                bullets(&proposal.acceptance_criteria, "- [ ] "),
                bullets(&proposal.scope_boundaries, "- "),
                evidence_markdown(report, &finding),
            );
            if let Some(rejection) = &finding.rejection {
                let previous = rejection
                    .decisions
                    .first()
                    .context("reconsideration is missing the rejection decision")?;
                let ticket = rejected
                    .iter()
                    .find(|ticket| ticket.id == previous.ticket_id)
                    .context("reconsideration is missing the rejected ticket")?;
                let reason = rejection
                    .reason
                    .as_ref()
                    .context("reconsideration is missing the rejection reason")?;
                body.push_str(&format!(
                    "\n\n## Prior rejection\n\n{}\n\nDecision recorded at {}. This proposal requires fresh approval.\n\n{}",
                    ticket.url,
                    previous.decided_at.to_rfc3339(),
                    bullets(&reason.text.lines().map(str::to_owned).collect::<Vec<_>>(), "> "),
                ));
            }
            Some(LinearWrite::Create {
                operation_id: uuid::Uuid::new_v4().to_string(),
                destination: route.destination.clone(),
                proposal_state_id: route.proposal_state_id.clone(),
                title: proposal.title,
                body,
            })
        }
        _ => {
            finding.reason = format!("The available evidence needs review. {}", finding.reason);
            None
        }
    };
    Ok((finding, write))
}

fn bounded_proposal(proposal: &Proposal) -> bool {
    proposal.uses_existing_infrastructure
        && [&proposal.title, &proposal.behavior]
            .iter()
            .all(|text| !text.trim().is_empty())
        && [
            &proposal.assumptions,
            &proposal.acceptance_criteria,
            &proposal.scope_boundaries,
        ]
        .iter()
        .all(|items| !items.is_empty() && items.iter().all(|item| !item.trim().is_empty()))
}

fn bullets(items: &[String], prefix: &str) -> String {
    items
        .iter()
        .map(|item| format!("{prefix}{item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn code_url(code: &CodeEvidence) -> String {
    let mut url = reqwest::Url::parse("https://github.com/").expect("valid GitHub URL");
    url.path_segments_mut()
        .expect("GitHub URL supports path segments")
        .pop_if_empty()
        .extend(code.repository.split('/'))
        .extend(["blob", &code.sha])
        .extend(code.path.split('/'));
    url.into()
}

fn evidence_markdown(report: &Report, finding: &Finding) -> String {
    let demand = &finding.need;
    let start: DateTime<Utc> = report.start;
    let end: DateTime<Utc> = report.end;
    let mut body = format!(
        "## Ship Signal evidence\n\nProduct: {}\n\nReport: {}\n\nInterval: {} to {} (end exclusive)\n\nNeed: {}\n\nDemand for the underlying need: {} of {} customers ({:.2}%)\n\n",
        report.product_id,
        report.id,
        start.to_rfc3339(),
        end.to_rfc3339(),
        demand.need,
        demand.customer_count,
        demand.denominator,
        demand.percentage,
    );
    if !demand.variants.is_empty() {
        body.push_str(&format!(
            "Requested variants: {}\n\nBehavior considered here: {}\n\n",
            demand.variants.join(", "),
            finding.variants.join(", ")
        ));
    }
    if let Some(repository) = &finding.repository {
        body.push_str(&format!("Repository: https://github.com/{repository}\n\n"));
    }
    let indices = demand
        .message_indices
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    body.push_str(&format!(
        "Supporting message indices in the saved report (zero-based): {indices}\n\n"
    ));
    body.push_str(&format!("Investigation: {}\n", finding.reason));
    for link in finding.tickets.iter().chain(&finding.code) {
        body.push_str(&format!("\n- {link}"));
    }
    body
}
