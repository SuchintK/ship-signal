use chrono::{DateTime, Utc};
use ship_signal::{AnalysisInput, Classification, Feedback, FeedbackSource, Need, Workflow};
use std::collections::BTreeMap;

struct Source(Vec<Feedback>);
impl FeedbackSource for Source {
    fn read(&mut self, _: DateTime<Utc>, _: DateTime<Utc>) -> anyhow::Result<AnalysisInput> {
        Ok(AnalysisInput {
            feedback: self.0.clone(),
            diagnostics: BTreeMap::new(),
        })
    }
}
fn feedback(id: &str, customer: Option<&str>) -> Feedback {
    Feedback {
        product_id: "shop".into(),
        feedback_source_id: "mail".into(),
        source_message_id: id.into(),
        customer_id: customer.map(str::to_string),
        submitted_at: "2026-11-06T09:00:00Z".parse().unwrap(),
        text: id.into(),
        source_reference: None,
    }
}
#[test]
fn completed_report_survives_changed_source_and_classifier() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite");
    let mut workflow = Workflow::open(&path)?;
    let mut source = Source(vec![feedback("1", Some("alice"))]);
    let report = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![Classification {
                message_index: 0,
                needs: vec![Need {
                    name: "export".into(),
                    variant: Some("CSV".into()),
                }],
            }])
        },
    )?;
    source.0.clear();
    drop(workflow);
    let workflow = Workflow::open(&path)?;
    assert_eq!(workflow.report(report.id)?, report);
    assert_eq!(report.products[0].denominator, 1);
    assert_eq!(report.products[0].needs[0].customer_count, 1);
    assert!(report.products[0].needs[0].qualifies);
    Ok(())
}

#[test]
fn counts_distinct_customers_per_product_and_preserves_variant_evidence() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    let mut rows = vec![
        feedback("a1", Some("alice")),
        feedback("a2", Some("alice")),
        feedback("b", Some("bob")),
        feedback("c", Some("carol")),
        feedback("d", Some("dave")),
        feedback("bug", Some("eve")),
        feedback("anonymous", None),
    ];
    rows[1].feedback_source_id = "chat".into();
    let mut source = Source(rows);
    let report = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            let needs = [
                vec![("export", Some("CSV"))],
                vec![("export", Some("CSV"))],
                vec![("export", Some("PDF")), ("dark mode", None)],
                vec![("other", None)],
                vec![("fourth", None)],
                vec![],
                vec![("export", Some("CSV"))],
            ];
            Ok(needs
                .into_iter()
                .enumerate()
                .map(|(message_index, needs)| Classification {
                    message_index,
                    needs: needs
                        .into_iter()
                        .map(|(name, variant)| Need {
                            name: name.into(),
                            variant: variant.map(str::to_string),
                        })
                        .collect(),
                })
                .collect())
        },
    )?;
    let product = &report.products[0];
    assert_eq!(product.denominator, 4);
    let export = product.needs.iter().find(|n| n.name == "export").unwrap();
    assert_eq!(
        (export.customer_count, export.percentage, export.qualifies),
        (2, 50.0, true)
    );
    assert_eq!(export.variants, vec!["CSV", "PDF"]);
    assert_eq!(export.message_indices, vec![0, 1, 2, 6]);
    let dark = product
        .needs
        .iter()
        .find(|n| n.name == "dark mode")
        .unwrap();
    assert_eq!(
        (dark.customer_count, dark.percentage, dark.qualifies),
        (1, 25.0, false)
    );
    Ok(())
}

#[test]
fn unidentified_and_bug_only_feedback_cannot_qualify() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    let mut source = Source(vec![
        feedback("bug", Some("alice")),
        feedback("anonymous", None),
    ]);
    let report = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        name: "export".into(),
                        variant: None,
                    }],
                },
            ])
        },
    )?;
    assert_eq!(report.products[0].denominator, 0);
    assert!(!report.products[0].needs[0].qualifies);
    Ok(())
}
