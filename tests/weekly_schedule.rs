use chrono::{DateTime, Utc};
use ship_signal::{
    AnalysisInput, Classification, Feedback, FeedbackSource, Need, WeeklyCycleStatus,
    WeeklyScheduler, Workflow,
};

fn utc(time: &str) -> DateTime<Utc> {
    time.parse().unwrap()
}

#[derive(Default)]
struct Source(Vec<Feedback>);

impl FeedbackSource for Source {
    fn read(
        &mut self,
        product: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> anyhow::Result<AnalysisInput> {
        let mut input = AnalysisInput {
            feedback: vec![],
            diagnostics: Default::default(),
        };
        for row in self
            .0
            .iter()
            .filter(|row| row.product_id == product && (start..end).contains(&row.submitted_at))
        {
            if row.text.is_empty() {
                *input.diagnostics.entry("empty text".into()).or_default() += 1;
            } else {
                input.feedback.push(row.clone());
            }
        }
        Ok(input)
    }
}

fn feedback(product: &str, id: &str, time: &str) -> Feedback {
    Feedback {
        product_id: product.into(),
        feedback_source_id: "mail".into(),
        source_message_id: id.into(),
        customer_id: Some("alice".into()),
        submitted_at: utc(time),
        text: id.into(),
        source_reference: None,
    }
}

fn classify(feedback: &[Feedback]) -> anyhow::Result<Vec<Classification>> {
    Ok((0..feedback.len())
        .map(|message_index| Classification {
            message_index,
            needs: vec![],
        })
        .collect())
}

#[test]
fn first_friday_analyzes_only_the_half_open_interval() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state.sqlite");
    let mut scheduler = WeeklyScheduler::open(&state, "shop", "Asia/Kolkata")?;
    let mut source = Source(vec![
        feedback("shop", "before start", "2026-11-13T03:29:59.999999Z"),
        feedback("shop", "at start", "2026-11-13T03:30:00Z"),
        feedback("shop", "after start", "2026-11-13T03:30:00.000001Z"),
        feedback("shop", "before end", "2026-11-20T03:29:59.999999Z"),
        feedback("shop", "at end", "2026-11-20T03:30:00Z"),
        feedback("shop", "after end", "2026-11-20T03:30:00.000001Z"),
    ]);
    let before = scheduler.tick(utc("2026-11-20T03:29:59Z"), &mut source, |_| {
        panic!("classification before the first Friday cutoff")
    })?;
    assert!(before.is_none());

    let cycle = scheduler
        .tick(utc("2026-11-20T03:30:00Z"), &mut source, classify)?
        .unwrap();
    let workflow = Workflow::open(&state)?;
    let report = workflow.report("shop", cycle.report_id.unwrap())?;
    let messages: Vec<_> = report
        .input
        .feedback
        .iter()
        .map(|row| row.text.as_str())
        .collect();
    assert_eq!(messages, ["at start", "after start", "before end"]);
    assert_eq!(report.start, utc("2026-11-13T03:30:00Z"));
    assert_eq!(report.end, utc("2026-11-20T03:30:00Z"));
    assert_eq!(cycle.approval_cutoff, utc("2026-11-21T03:30:00Z"));
    Ok(())
}

#[test]
fn multiweek_outage_catches_up_only_the_latest_due_cycle() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state.sqlite");
    let mut scheduler = WeeklyScheduler::open(&state, "shop", "UTC")?;
    let mut source = Source(vec![
        feedback("shop", "from a skipped week", "2026-11-24T12:00:00Z"),
        feedback("shop", "from the latest week", "2026-12-01T12:00:00Z"),
        feedback("shop", "from the next week", "2026-12-05T12:00:00Z"),
    ]);
    assert!(
        scheduler
            .tick(utc("2026-11-13T09:00:01Z"), &mut source, |_| {
                panic!("first setup after the cutoff must wait until next Friday")
            })?
            .is_none()
    );
    drop(scheduler);

    let mut scheduler = WeeklyScheduler::open(&state, "shop", "UTC")?;
    let cycle = scheduler
        .tick(utc("2026-12-07T12:00:00Z"), &mut source, classify)?
        .unwrap();
    assert_eq!(cycle.feedback_start, utc("2026-11-27T09:00:00Z"));
    assert_eq!(cycle.analysis_cutoff, utc("2026-12-04T09:00:00Z"));
    assert_eq!(cycle.approval_cutoff, utc("2026-12-05T09:00:00Z"));

    let workflow = Workflow::open(&state)?;
    let report = workflow.report("shop", cycle.report_id.unwrap())?;
    assert_eq!(report.input.feedback.len(), 1);
    assert_eq!(report.input.feedback[0].text, "from the latest week");
    let cycles = workflow.weekly_cycles("shop")?;
    assert_eq!(
        cycles.iter().map(|cycle| cycle.status).collect::<Vec<_>>(),
        [
            WeeklyCycleStatus::Planned,
            WeeklyCycleStatus::Completed,
            WeeklyCycleStatus::Skipped,
            WeeklyCycleStatus::Skipped
        ],
    );
    assert_eq!(cycles[2].analysis_cutoff, utc("2026-11-27T09:00:00Z"));
    assert_eq!(cycles[3].analysis_cutoff, utc("2026-11-20T09:00:00Z"));
    assert!(cycles[2..].iter().all(|cycle| cycle.report_id.is_none()));
    Ok(())
}

#[test]
fn timezone_change_keeps_the_due_cycle_and_moves_the_next_cutoff() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state.sqlite");
    let mut scheduler = WeeklyScheduler::open(&state, "shop", "UTC")?;
    let mut source = Source::default();
    assert!(
        scheduler
            .tick(utc("2026-11-12T18:00:00Z"), &mut source, classify)?
            .is_none()
    );
    drop(scheduler);

    let mut scheduler = WeeklyScheduler::open(&state, "shop", "Asia/Kolkata")?;
    let original = scheduler
        .tick(utc("2026-11-14T11:00:00Z"), &mut source, classify)?
        .unwrap();
    assert_eq!(original.timezone, "UTC");
    assert_eq!(original.analysis_cutoff, utc("2026-11-13T09:00:00Z"));
    assert_eq!(original.approval_cutoff, utc("2026-11-14T09:00:00Z"));

    source.0.push(feedback(
        "shop",
        "at the previous cutoff",
        "2026-11-13T09:00:00Z",
    ));
    assert!(
        scheduler
            .tick(utc("2026-11-20T03:29:59Z"), &mut source, classify)?
            .is_none()
    );
    let next = scheduler
        .tick(utc("2026-11-20T03:30:00Z"), &mut source, classify)?
        .unwrap();
    assert_eq!(next.timezone, "Asia/Kolkata");
    assert_eq!(next.feedback_start, original.analysis_cutoff);
    assert_eq!(next.approval_cutoff, utc("2026-11-21T03:30:00Z"));
    let report = Workflow::open(&state)?.report("shop", next.report_id.unwrap())?;
    assert_eq!(report.input.feedback[0].text, "at the previous cutoff");
    Ok(())
}

#[test]
fn changing_timezone_before_the_first_cutoff_uses_the_new_zones_previous_friday()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state.sqlite");
    let mut source = Source(vec![feedback(
        "shop",
        "early feedback",
        "2026-11-06T04:00:00Z",
    )]);
    let mut scheduler = WeeklyScheduler::open(&state, "shop", "UTC")?;
    scheduler.tick(utc("2026-11-09T09:00:00Z"), &mut source, classify)?;
    drop(scheduler);

    let mut scheduler = WeeklyScheduler::open(&state, "shop", "Asia/Kolkata")?;
    scheduler.tick(utc("2026-11-10T09:00:00Z"), &mut source, classify)?;
    let cycle = scheduler
        .tick(utc("2026-11-13T03:30:00Z"), &mut source, classify)?
        .unwrap();
    assert_eq!(cycle.feedback_start, utc("2026-11-06T03:30:00Z"));
    let report = Workflow::open(&state)?.report("shop", cycle.report_id.unwrap())?;
    assert_eq!(report.input.feedback[0].text, "early feedback");
    Ok(())
}

#[test]
fn timezone_change_does_not_backdate_an_earlier_friday_cutoff() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state.sqlite");
    let mut scheduler = WeeklyScheduler::open(&state, "shop", "UTC")?;
    let mut source = Source::default();
    scheduler.tick(utc("2026-11-12T18:00:00Z"), &mut source, classify)?;
    drop(scheduler);

    let mut scheduler = WeeklyScheduler::open(&state, "shop", "Asia/Kolkata")?;
    assert!(
        scheduler
            .tick(utc("2026-11-13T06:00:00Z"), &mut source, classify)?
            .is_none()
    );
    let current = scheduler
        .tick(utc("2026-11-13T09:00:00Z"), &mut source, classify)?
        .unwrap();
    assert_eq!(current.timezone, "UTC");
    let next = scheduler
        .tick(utc("2026-11-20T03:30:00Z"), &mut source, classify)?
        .unwrap();
    assert_eq!(next.timezone, "Asia/Kolkata");
    assert_eq!(next.feedback_start, current.analysis_cutoff);
    Ok(())
}

#[test]
fn each_cutoff_uses_its_local_daylight_saving_offset() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state.sqlite");
    let mut source = Source::default();
    let mut scheduler = WeeklyScheduler::open(&state, "shop", "America/New_York")?;
    let before = scheduler
        .tick(utc("2026-03-06T14:00:00Z"), &mut source, classify)?
        .unwrap();
    assert_eq!(before.approval_cutoff, utc("2026-03-07T14:00:00Z"));
    drop(scheduler);

    let mut scheduler = WeeklyScheduler::open(&state, "shop", "America/New_York")?;
    assert!(
        scheduler
            .tick(utc("2026-03-13T12:59:59Z"), &mut source, classify)?
            .is_none()
    );
    let after = scheduler
        .tick(utc("2026-03-13T13:00:00Z"), &mut source, classify)?
        .unwrap();
    assert_eq!(after.feedback_start, utc("2026-03-06T14:00:00Z"));
    assert_eq!(after.analysis_cutoff, utc("2026-03-13T13:00:00Z"));
    assert_eq!(after.approval_cutoff, utc("2026-03-14T13:00:00Z"));
    Ok(())
}

#[test]
fn first_cutoff_moves_forward_if_the_local_friday_was_skipped() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state.sqlite");
    let mut source = Source::default();
    let mut scheduler = WeeklyScheduler::open(&state, "shop", "Pacific/Apia")?;
    // Apia advanced from Thursday December 29 directly to Saturday December 31.
    assert!(
        scheduler
            .tick(utc("2011-12-30T16:00:00Z"), &mut source, classify)?
            .is_none()
    );
    let cycle = scheduler
        .tick(utc("2011-12-30T19:00:00Z"), &mut source, classify)?
        .unwrap();
    assert_eq!(cycle.friday.to_string(), "2011-12-30");
    assert_eq!(cycle.analysis_cutoff, utc("2011-12-30T19:00:00Z"));
    Ok(())
}

struct UnavailableSource;

impl FeedbackSource for UnavailableSource {
    fn read(
        &mut self,
        _: &str,
        _: DateTime<Utc>,
        _: DateTime<Utc>,
    ) -> anyhow::Result<AnalysisInput> {
        anyhow::bail!("source unavailable")
    }
}

#[test]
fn failure_retries_original_cycle_and_completion_survives_restart() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state.sqlite");
    let cutoff = utc("2026-11-13T09:00:00Z");
    let mut scheduler = WeeklyScheduler::open(&state, "shop", "UTC")?;
    assert!(WeeklyScheduler::open(&state, "shop", "UTC").is_err());
    scheduler
        .tick(cutoff, &mut UnavailableSource, |_| {
            panic!("source failure must stop classification")
        })
        .unwrap_err();
    let workflow = Workflow::open(&state)?;
    let cycles = workflow.weekly_cycles("shop")?;
    assert_eq!(cycles[1].status, WeeklyCycleStatus::Pending);
    assert_eq!(cycles[1].report_id, None);
    drop(scheduler);

    let mut source = Source(vec![feedback("shop", "request", "2026-11-12T00:00:00Z")]);
    let mut scheduler = WeeklyScheduler::open(&state, "shop", "UTC")?;
    scheduler
        .tick(utc("2026-11-14T10:00:00Z"), &mut source, |_| {
            anyhow::bail!("classifier unavailable")
        })
        .unwrap_err();
    assert_eq!(workflow.weekly_cycles("shop")?, cycles);
    let completed = scheduler
        .tick(utc("2026-11-14T11:00:00Z"), &mut source, |_| {
            Ok(vec![Classification {
                message_index: 0,
                needs: vec![Need {
                    need: "Export orders".into(),
                    variant: Some("CSV".into()),
                }],
            }])
        })?
        .unwrap();
    assert_eq!(completed.analysis_cutoff, cutoff);
    assert_eq!(completed.approval_cutoff, utc("2026-11-14T09:00:00Z"));
    let report = workflow.report("shop", completed.report_id.unwrap())?;
    assert_eq!(report.products[0].denominator, 1);
    drop(scheduler);

    let mut scheduler = WeeklyScheduler::open(&state, "shop", "UTC")?;
    let completed_cycles = workflow.weekly_cycles("shop")?;
    assert!(
        scheduler
            .tick(utc("2026-11-15T08:00:00Z"), &mut UnavailableSource, |_| {
                panic!("completed cycles must not run again")
            })?
            .is_none()
    );
    assert_eq!(workflow.weekly_cycles("shop")?, completed_cycles);
    assert_eq!(workflow.report("shop", report.id)?, report);
    Ok(())
}

#[test]
fn a_failed_completion_rolls_back_the_report_insert() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state.sqlite");
    let cutoff = utc("2026-11-13T09:00:00Z");
    let mut scheduler = WeeklyScheduler::open(&state, "shop", "UTC")?;
    let storage = rusqlite::Connection::open(&state)?;
    storage.execute_batch(
        "CREATE TRIGGER fail_completion BEFORE UPDATE ON weekly_cycles
         WHEN NEW.status = 'completed'
         BEGIN SELECT RAISE(ABORT, 'simulated completion failure'); END;",
    )?;

    let mut source = Source::default();
    scheduler.tick(cutoff, &mut source, classify).unwrap_err();
    let workflow = Workflow::open(&state)?;
    assert!(workflow.report("shop", 1).is_err());
    assert_eq!(
        workflow.weekly_cycles("shop")?[1].status,
        WeeklyCycleStatus::Pending
    );

    storage.execute_batch("DROP TRIGGER fail_completion")?;
    let cycle = scheduler.tick(cutoff, &mut source, classify)?.unwrap();
    assert_eq!(
        workflow.report("shop", cycle.report_id.unwrap())?.end,
        cutoff
    );
    Ok(())
}

#[test]
fn products_sharing_state_keep_feedback_diagnostics_and_empty_reports_separate()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state.sqlite");
    let cutoff = utc("2026-11-13T09:00:00Z");
    let mut invalid = feedback("other", "shared", "2026-11-12T00:00:00Z");
    invalid.text.clear();
    let mut source = Source(vec![
        feedback("shop", "shared", "2026-11-12T00:00:00Z"),
        invalid,
    ]);
    let mut scheduler = WeeklyScheduler::open(&state, "shop", "UTC")?;
    let shop = scheduler
        .tick(cutoff, &mut source, |input| {
            assert_eq!(input.len(), 1);
            assert_eq!(input[0].product_id, "shop");
            classify(input)
        })?
        .unwrap();
    drop(scheduler);
    let mut scheduler = WeeklyScheduler::open(&state, "other", "UTC")?;
    let other = scheduler
        .tick(cutoff, &mut source, |input| {
            assert!(input.is_empty());
            classify(input)
        })?
        .unwrap();

    let workflow = Workflow::open(&state)?;
    let shop_report = workflow.report("shop", shop.report_id.unwrap())?;
    let other_report = workflow.report("other", other.report_id.unwrap())?;
    assert!(shop_report.input.diagnostics.is_empty());
    assert_eq!(other_report.product_id, "other");
    assert!(other_report.input.feedback.is_empty());
    assert_eq!(other_report.input.diagnostics.get("empty text"), Some(&1));
    assert!(workflow.report("shop", other_report.id).is_err());
    assert!(workflow.report("other", shop_report.id).is_err());
    assert!(
        workflow
            .weekly_cycles("shop")?
            .iter()
            .all(|cycle| cycle.product_id == "shop")
    );
    assert!(
        workflow
            .weekly_cycles("other")?
            .iter()
            .all(|cycle| cycle.product_id == "other")
    );
    Ok(())
}
