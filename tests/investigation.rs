use anyhow::Result;
use chrono::{DateTime, Utc};
use ship_signal::{
    AnalysisInput, Classification, Feedback, FeedbackSource, Need, Workflow,
    investigation::{
        Assessment, AssessmentKind, CodeEvidence, CodeSelection, Finding, FindingKind,
        InvestigationConfig, InvestigationModel, InvestigationSources, LinearReference,
        LinearScope, LinearWrite, ProductConfig, Proposal, PullRequestEvidence, RepositoryRoute,
        RepositorySnapshot, SearchContext, Ticket, WriteStatus,
    },
};

struct Source(Vec<Feedback>);

impl FeedbackSource for Source {
    fn read(&mut self, product: &str, _: DateTime<Utc>, _: DateTime<Utc>) -> Result<AnalysisInput> {
        Ok(AnalysisInput {
            feedback: self
                .0
                .iter()
                .filter(|row| row.product_id == product)
                .cloned()
                .collect(),
            diagnostics: Default::default(),
        })
    }
}

fn report(workflow: &mut Workflow, product: &str) -> Result<i64> {
    let rows = (0..4)
        .map(|i| Feedback {
            product_id: product.into(),
            feedback_source_id: "mail".into(),
            source_message_id: i.to_string(),
            customer_id: Some(format!("customer-{i}")),
            submitted_at: "2026-11-13T08:00:00Z".parse().unwrap(),
            text: format!("{product} private message {i}"),
            source_reference: None,
        })
        .collect();
    Ok(workflow
        .analyze(
            product,
            &mut Source(rows),
            "2026-11-06T09:00:00Z".parse()?,
            "2026-11-13T09:00:00Z".parse()?,
            |_| {
                Ok([
                    ("Stock alerts", Some("Email")),
                    ("Stock alerts", Some("SMS")),
                    ("History", None),
                    ("Accessibility", None),
                ]
                .into_iter()
                .enumerate()
                .map(|(message_index, (need, variant))| Classification {
                    message_index,
                    needs: vec![Need {
                        need: need.into(),
                        variant: variant.map(str::to_owned),
                    }],
                })
                .collect())
            },
        )?
        .id)
}

fn config() -> InvestigationConfig {
    let product = |product: &str| ProductConfig {
        product_id: product.into(),
        scopes: vec![LinearScope {
            team_id: format!("team-{product}"),
            project_id: None,
        }],
        labels: vec!["customer-demand".into()],
        not_needed_state_ids: vec!["rejected".into()],
        repositories: vec![RepositoryRoute {
            repository: format!("example/{product}"),
            base_ref: "main".into(),
            live_ref: Some("production".into()),
            search_paths: vec![],
            destination: LinearScope {
                team_id: format!("team-{product}"),
                project_id: None,
            },
            proposal_state_id: "backlog".into(),
        }],
    };
    InvestigationConfig {
        products: vec![product("shop"), product("other")],
    }
}

#[derive(Default)]
struct Services {
    reads: usize,
    catalog_reads: usize,
    updated_catalog: Option<Vec<Ticket>>,
    writes: Vec<LinearWrite>,
    tickets: Vec<Ticket>,
    fail_write: bool,
    change_scope_before_write: bool,
}

impl InvestigationSources for Services {
    fn tickets(&mut self, _: &ProductConfig) -> Result<Vec<Ticket>> {
        self.reads += 1;
        self.catalog_reads += 1;
        if self.catalog_reads > 1
            && let Some(updated) = &self.updated_catalog
        {
            return Ok(updated.clone());
        }
        Ok(self.tickets.clone())
    }

    fn ticket(&mut self, id: &str) -> Result<Ticket> {
        let mut ticket = self
            .tickets
            .iter()
            .find(|ticket| ticket.id == id)
            .unwrap()
            .clone();
        if self.change_scope_before_write {
            ticket.description = "Acceptance criteria changed.".into();
        }
        Ok(ticket)
    }

    fn pull_requests(
        &mut self,
        _: &ProductConfig,
        _: &[Ticket],
    ) -> Result<Vec<PullRequestEvidence>> {
        Ok(vec![])
    }

    fn repository(&mut self, route: &RepositoryRoute) -> Result<RepositorySnapshot> {
        self.reads += 1;
        Ok(RepositorySnapshot {
            repository: route.repository.clone(),
            base_sha: "a".repeat(40),
            live_sha: Some("b".repeat(40)),
            base_paths: vec!["alerts.rs".into()],
            live_paths: vec!["alerts.rs".into()],
        })
    }

    fn code(
        &mut self,
        snapshot: &RepositorySnapshot,
        selection: &CodeSelection,
    ) -> Result<CodeEvidence> {
        Ok(CodeEvidence {
            repository: snapshot.repository.clone(),
            path: selection.path.clone(),
            sha: if selection.live { "b" } else { "a" }.repeat(40),
            live: selection.live,
            text: "Email alert delivery uses the inventory event.".into(),
        })
    }

    fn write(&mut self, write: &LinearWrite) -> Result<LinearReference> {
        self.writes.push(write.clone());
        anyhow::ensure!(!self.fail_write, "response lost");
        let id = match write {
            LinearWrite::Comment { operation_id, .. }
            | LinearWrite::Create { operation_id, .. } => operation_id.clone(),
        };
        Ok(LinearReference {
            id,
            url: "https://linear.app/example/issue/write".into(),
        })
    }
}

#[derive(Default)]
struct Model {
    calls: usize,
    findings: Vec<Assessment>,
    live_code: bool,
}

impl InvestigationModel for Model {
    fn select_code(&mut self, context: &SearchContext) -> Result<Vec<CodeSelection>> {
        self.calls += 1;
        assert!(
            context
                .tickets
                .iter()
                .all(|ticket| ticket.scope.team_id == format!("team-{}", context.product_id))
        );
        assert_eq!(context.demand.need, "Stock alerts");
        assert_eq!(context.demand.customer_count, 2);
        assert_eq!(context.demand.denominator, 4);
        assert_eq!(context.demand.variants, ["Email", "SMS"]);
        Ok(vec![CodeSelection {
            repository: context.repositories[0].repository.clone(),
            path: "alerts.rs".into(),
            live: self.live_code,
        }])
    }

    fn assess(&mut self, _: &SearchContext, _: &[CodeEvidence]) -> Result<Vec<Assessment>> {
        self.calls += 1;
        Ok(self.findings.clone())
    }
}

#[test]
fn provisional_and_wrong_product_reports_stop_before_investigation() -> Result<()> {
    let state = tempfile::NamedTempFile::new()?;
    let mut workflow = Workflow::open(state.path())?;
    let own = report(&mut workflow, "shop")?;
    let other = report(&mut workflow, "other")?;
    let mut services = Services::default();
    let mut model = Model::default();

    assert!(
        workflow
            .investigate("shop", other, &config(), &mut services, &mut model)
            .is_err()
    );
    let provisional = workflow.investigate("shop", own, &config(), &mut services, &mut model)?;
    assert!(provisional.needs_identity_review);
    assert_eq!(services.reads, 0);
    assert_eq!(model.calls, 0);

    for need in ["Stock alerts", "History", "Accessibility"] {
        workflow.assign_need_identity("shop", need, need)?;
    }
    let reviewed = workflow.review_report("shop", own)?;
    let finished =
        workflow.investigate("shop", reviewed.id, &config(), &mut services, &mut model)?;
    assert_eq!(finished.report_id, reviewed.id);
    assert!(!finished.needs_identity_review);
    assert!(
        finished
            .findings
            .iter()
            .all(|finding: &Finding| finding.kind == FindingKind::HumanReview)
    );
    assert_eq!(model.calls, 2);
    assert!(services.writes.is_empty());
    Ok(())
}

fn assign_identities(workflow: &Workflow, product: &str) -> Result<()> {
    for need in ["Stock alerts", "History", "Accessibility"] {
        workflow.assign_need_identity(product, need, need)?;
    }
    Ok(())
}

fn email_ticket(state_id: &str, state_type: &str) -> Ticket {
    Ticket {
        id: "issue-email".into(),
        url: "https://linear.app/example/issue/EMAIL-1".into(),
        scope: LinearScope {
            team_id: "team-shop".into(),
            project_id: None,
        },
        title: "Stock alerts".into(),
        description: "## Acceptance criteria\nSend an email when an item comes back in stock."
            .into(),
        state_id: state_id.into(),
        state_name: "Email status".into(),
        state_type: state_type.into(),
        archived: false,
        labels: vec![],
        pull_request_urls: vec![],
    }
}

fn assessment(kind: AssessmentKind, variant_index: usize) -> Assessment {
    Assessment {
        kind,
        variant_indices: vec![variant_index],
        repository: Some("example/shop".into()),
        matching_ticket_ids: vec![],
        related_ticket_ids: vec![],
        code_indices: vec![0],
        reason: "The evidence covers the selected delivery channel.".into(),
        proposal: None,
    }
}

fn scoped_findings() -> Vec<Assessment> {
    let mut email = assessment(AssessmentKind::MatchingWork, 0);
    email.matching_ticket_ids = vec!["issue-email".into()];
    let mut sms = assessment(AssessmentKind::DistinctWork, 1);
    sms.related_ticket_ids = vec!["issue-email".into()];
    sms.proposal = Some(Proposal {
        title: "Send stock alert by SMS".into(),
        behavior: "Send SMS via the existing notification system.".into(),
        assumptions: vec!["The subscriber has opted into SMS.".into()],
        acceptance_criteria: vec!["An SMS is sent after the product comes back in stock.".into()],
        scope_boundaries: vec!["Leave email settings as they are.".into()],
        uses_existing_infrastructure: true,
    });
    vec![email, sms]
}

#[test]
fn updates_matching_behavior_and_creates_linked_work_for_a_distinct_variant() -> Result<()> {
    let state = tempfile::NamedTempFile::new()?;
    let mut workflow = Workflow::open(state.path())?;
    assign_identities(&workflow, "shop")?;
    let own = report(&mut workflow, "shop")?;
    assign_identities(&workflow, "other")?;
    let other = report(&mut workflow, "other")?;
    let mut foreign = email_ticket("approved", "started");
    foreign.scope.team_id = "team-other".into();
    foreign.id = "other-issue".into();
    let email = email_ticket("approved", "started");
    let mut services = Services {
        tickets: vec![email.clone(), foreign],
        ..Default::default()
    };
    let mut model = Model {
        findings: scoped_findings(),
        ..Default::default()
    };

    assert!(
        workflow
            .investigate("shop", other, &config(), &mut services, &mut model)
            .is_err()
    );
    let result = workflow.investigate("shop", own, &config(), &mut services, &mut model)?;
    assert_eq!(result.findings.len(), 2);
    assert_eq!(result.findings[0].kind, FindingKind::MatchingWork);
    assert_eq!(result.findings[1].kind, FindingKind::Proposal);
    assert!(
        result
            .findings
            .iter()
            .all(|finding| matches!(finding.write, Some(WriteStatus::Completed { .. })))
    );

    let [
        LinearWrite::Comment {
            ticket,
            body: comment,
            ..
        },
        LinearWrite::Create {
            destination,
            title,
            body: proposal,
            ..
        },
    ] = services.writes.as_slice()
    else {
        panic!("expected one evidence comment and one proposal")
    };
    assert_eq!(**ticket, email);
    assert_eq!(destination.team_id, "team-shop");
    assert_eq!(title, "Send stock alert by SMS");
    for body in [comment, proposal] {
        assert!(body.contains("2 of 4 customers (50.00%)"));
        assert!(body.contains("Requested variants: Email, SMS"));
        assert!(body.contains("(zero-based): 0, 1"));
        assert!(!body.contains("private message") && !body.contains("customer-0"));
    }
    assert!(comment.contains("Behavior considered here: Email"));
    assert!(proposal.contains("Behavior considered here: SMS"));
    assert!(proposal.contains(&email.url));

    let replay = Workflow::open(state.path())?.investigate(
        "shop",
        own,
        &config(),
        &mut services,
        &mut model,
    )?;
    assert_eq!(
        replay.findings[0].operation_id,
        result.findings[0].operation_id
    );
    assert_eq!(services.writes.len(), 2);
    assert_eq!(model.calls, 2);
    Ok(())
}

#[test]
fn holds_uncertain_writes_after_restart_and_leaves_other_products_independent() -> Result<()> {
    let state = tempfile::NamedTempFile::new()?;
    let mut workflow = Workflow::open(state.path())?;
    assign_identities(&workflow, "shop")?;
    let own = report(&mut workflow, "shop")?;
    let mut services = Services {
        tickets: vec![email_ticket("started", "started")],
        fail_write: true,
        ..Default::default()
    };
    let mut model = Model {
        findings: scoped_findings(),
        ..Default::default()
    };
    let result = workflow.investigate("shop", own, &config(), &mut services, &mut model)?;
    assert!(matches!(
        result.findings[0].write,
        Some(WriteStatus::Uncertain)
    ));
    assert_eq!(result.findings[1].kind, FindingKind::HumanReview);

    let mut workflow = Workflow::open(state.path())?;
    services.fail_write = false;
    let reload = workflow.investigate("shop", own, &config(), &mut services, &mut model)?;
    assert_eq!(
        result.findings[0].operation_id,
        reload.findings[0].operation_id
    );
    assert_eq!(services.writes.len(), 1);

    let mut overlapping = config();
    let other = &mut overlapping.products[1];
    other.scopes = vec![LinearScope {
        team_id: "team-shop".into(),
        project_id: None,
    }];
    other.repositories[0].destination = other.scopes[0].clone();
    overlapping.products.remove(0);
    assign_identities(&workflow, "other")?;
    let other_id = report(&mut workflow, "other")?;
    assert!(
        workflow
            .investigate("other", other_id, &overlapping, &mut services, &mut model)
            .is_err()
    );
    assert_eq!(services.writes.len(), 1);

    let mut other_findings = scoped_findings();
    for decision in &mut other_findings {
        decision.repository = Some("example/other".into());
        for id in decision
            .matching_ticket_ids
            .iter_mut()
            .chain(&mut decision.related_ticket_ids)
        {
            *id = "other-issue".into();
        }
    }
    services.tickets[0].scope.team_id = "team-other".into();
    services.tickets[0].id = "other-issue".into();
    model.findings = other_findings;
    let independent =
        workflow.investigate("other", other_id, &config(), &mut services, &mut model)?;
    assert!(matches!(
        independent.findings[0].write,
        Some(WriteStatus::Completed { .. })
    ));
    assert_eq!(services.writes.len(), 3);
    Ok(())
}

#[test]
fn rejects_same_scope_and_requires_live_evidence_for_already_available_behavior() -> Result<()> {
    let state = tempfile::NamedTempFile::new()?;
    let mut workflow = Workflow::open(state.path())?;
    assign_identities(&workflow, "shop")?;
    let first = report(&mut workflow, "shop")?;
    let mut rejected = email_ticket("rejected", "canceled");
    rejected.archived = true;
    let mut services = Services {
        tickets: vec![rejected],
        ..Default::default()
    };
    let mut email = assessment(AssessmentKind::MatchingWork, 0);
    email.matching_ticket_ids = vec!["issue-email".into()];
    let sms = assessment(AssessmentKind::ExistingFunctionality, 1);
    let mut model = Model {
        findings: vec![email, sms],
        ..Default::default()
    };
    let base = workflow.investigate("shop", first, &config(), &mut services, &mut model)?;
    assert_eq!(base.findings[0].kind, FindingKind::RejectedWork);
    assert_eq!(base.findings[1].kind, FindingKind::HumanReview);

    let second = report(&mut workflow, "shop")?;
    model.live_code = true;
    let live = workflow.investigate("shop", second, &config(), &mut services, &mut model)?;
    assert_eq!(
        live.findings[1].kind,
        FindingKind::PossibleBugOrDiscoverability
    );
    assert!(live.findings[1].code[0].contains(&"b".repeat(40)));
    assert!(services.writes.is_empty());
    Ok(())
}

#[test]
fn ambiguous_ownership_and_ticket_scope_changes_require_review() -> Result<()> {
    let state = tempfile::NamedTempFile::new()?;
    let mut workflow = Workflow::open(state.path())?;
    assign_identities(&workflow, "shop")?;
    let report = report(&mut workflow, "shop")?;
    let mut services = Services {
        tickets: vec![email_ticket("approved", "started")],
        change_scope_before_write: true,
        ..Default::default()
    };
    let mut findings = scoped_findings();
    findings[1].repository = None;
    let mut model = Model {
        findings,
        ..Default::default()
    };
    let result = workflow.investigate("shop", report, &config(), &mut services, &mut model)?;
    assert!(
        result
            .findings
            .iter()
            .all(|finding| finding.kind == FindingKind::HumanReview)
    );
    assert!(services.writes.is_empty());
    Ok(())
}

#[test]
fn current_completed_cycle_can_investigate_after_scheduler_restart() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state.sqlite");
    assign_identities(&Workflow::open(&state)?, "shop")?;
    let mut scheduler = ship_signal::WeeklyScheduler::open(&state, "shop", "UTC")?;
    let friday = "2026-11-13T09:00:00Z".parse()?;
    let mut source = Source(vec![]);
    let mut services = Services::default();
    let mut model = Model::default();

    assert!(
        scheduler
            .investigate_due(friday, &config(), &mut services, &mut model)?
            .is_none()
    );
    let cycle = scheduler
        .tick(friday, &mut source, |_| Ok(vec![]))?
        .unwrap();
    drop(scheduler);
    let scheduler = ship_signal::WeeklyScheduler::open(&state, "shop", "UTC")?;
    let result = scheduler
        .investigate_due(friday, &config(), &mut services, &mut model)?
        .unwrap();
    assert_eq!(Some(result.report_id), cycle.report_id);
    assert!(result.findings.is_empty());
    assert_eq!(services.reads, 0);
    assert!(
        scheduler
            .investigate_due(friday, &config(), &mut services, &mut model)?
            .is_none()
    );
    Ok(())
}

#[test]
fn explicit_cli_reports_provisional_demand_without_provider_credentials() -> Result<()> {
    let state = tempfile::NamedTempFile::new()?;
    let mut workflow = Workflow::open(state.path())?;
    let report_id = report(&mut workflow, "shop")?;
    let config = tempfile::NamedTempFile::new()?;
    std::fs::write(
        config.path(),
        r#"{"products":[{"product_id":"shop",
      "scopes":[{"team_id":"team-shop","project_id":null}],
      "repositories":[{"repository":"example/shop","base_ref":"main",
        "destination":{"team_id":"team-shop","project_id":null},
        "proposal_state_id":"backlog"}]}]}"#,
    )?;
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ship-signal"))
        .args([
            "investigate",
            "--product",
            "shop",
            "--report",
            &report_id.to_string(),
            "--model",
            "controlled",
            "--config",
        ])
        .arg(config.path())
        .arg("--state")
        .arg(state.path())
        .env_remove("OPENAI_API_KEY")
        .env_remove("LINEAR_API_KEY")
        .env_remove("GITHUB_TOKEN")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(result["product_id"], "shop");
    assert_eq!(result["report_id"], report_id);
    assert_eq!(result["needs_identity_review"], true);
    Ok(())
}

#[test]
fn skips_a_proposal_if_linear_work_changed_since_investigation() -> Result<()> {
    let state = tempfile::NamedTempFile::new()?;
    let mut workflow = Workflow::open(state.path())?;
    assign_identities(&workflow, "shop")?;
    let report_id = report(&mut workflow, "shop")?;
    let email = email_ticket("started", "started");
    let mut new_sms = email_ticket("planned", "unstarted");
    new_sms.id = "issue-sms".into();
    new_sms.description = "Existing SMS proposal covering the same behavior".into();
    let mut services = Services {
        tickets: vec![email.clone()],
        updated_catalog: Some(vec![email, new_sms]),
        ..Default::default()
    };
    let mut model = Model {
        findings: scoped_findings(),
        ..Default::default()
    };

    let result = workflow.investigate("shop", report_id, &config(), &mut services, &mut model)?;
    assert!(matches!(
        result.findings[0].write,
        Some(WriteStatus::Completed { .. })
    ));
    assert_eq!(result.findings[1].kind, FindingKind::HumanReview);
    assert_eq!(services.writes.len(), 1);
    Ok(())
}

#[test]
fn moving_a_ticket_between_product_scopes_cannot_mix_evidence() -> Result<()> {
    let state = tempfile::NamedTempFile::new()?;
    let mut workflow = Workflow::open(state.path())?;
    assign_identities(&workflow, "shop")?;
    let own = report(&mut workflow, "shop")?;
    let mut services = Services {
        tickets: vec![email_ticket("started", "started")],
        ..Default::default()
    };
    let mut model = Model {
        findings: scoped_findings(),
        ..Default::default()
    };
    workflow.investigate("shop", own, &config(), &mut services, &mut model)?;
    assert_eq!(services.writes.len(), 2);

    assign_identities(&workflow, "other")?;
    let other = report(&mut workflow, "other")?;
    services.tickets[0].scope.team_id = "team-other".into();
    for decision in &mut model.findings {
        decision.repository = Some("example/other".into());
    }
    let result = workflow.investigate("other", other, &config(), &mut services, &mut model)?;
    assert_eq!(model.calls, 2);
    assert_eq!(result.findings[0].kind, FindingKind::HumanReview);
    assert_eq!(services.writes.len(), 2);
    Ok(())
}
