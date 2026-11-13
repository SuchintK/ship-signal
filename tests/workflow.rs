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

#[test]
fn confirmed_need_identity_connects_labels_across_reports_without_changing_old_evidence()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite");
    let mut workflow = Workflow::open(&path)?;
    let mut source = Source(vec![feedback("first", Some("alice"))]);
    let classify = |name: &'static str| {
        move |_: &[Feedback]| {
            Ok(vec![Classification {
                message_index: 0,
                needs: vec![Need {
                    name: name.into(),
                    variant: Some("CSV".into()),
                }],
            }])
        }
    };
    let old = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        classify("CSV export"),
    )?;
    assert_eq!(old.products[0].needs[0].identity, None);
    workflow.assign_need_identity("shop", "CSV export", "export-data")?;
    workflow.assign_need_identity("shop", "download data as CSV", "export-data")?;
    drop(workflow);
    let mut workflow = Workflow::open(&path)?;
    let first = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        classify("CSV export"),
    )?;
    let second = workflow.analyze(
        &mut source,
        "2026-11-13T00:00:00Z".parse()?,
        "2026-11-20T00:00:00Z".parse()?,
        classify("download data as CSV"),
    )?;
    assert_eq!(
        first.products[0].needs[0].identity.as_deref(),
        Some("export-data")
    );
    assert_eq!(
        second.products[0].needs[0].identity.as_deref(),
        Some("export-data")
    );
    assert_eq!(workflow.report(old.id)?, old);
    assert!(
        workflow
            .assign_need_identity("shop", "CSV export", "different-need")
            .is_err()
    );
    Ok(())
}

#[test]
fn confirmed_aliases_combine_demand_only_within_the_same_product() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    workflow.assign_need_identity("shop", "CSV export", "export-data")?;
    workflow.assign_need_identity("shop", "download data as CSV", "export-data")?;
    let mut another_product = feedback("third", Some("alice"));
    another_product.product_id = "other-shop".into();
    let mut source = Source(vec![
        feedback("first", Some("alice")),
        feedback("second", Some("bob")),
        another_product,
    ]);
    let report = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![Need {
                        name: "CSV export".into(),
                        variant: Some("CSV".into()),
                    }],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        name: "download data as CSV".into(),
                        variant: Some("CSV".into()),
                    }],
                },
                Classification {
                    message_index: 2,
                    needs: vec![Need {
                        name: "CSV export".into(),
                        variant: None,
                    }],
                },
            ])
        },
    )?;
    let shop = report
        .products
        .iter()
        .find(|p| p.product_id == "shop")
        .unwrap();
    assert_eq!(shop.needs.len(), 1);
    assert_eq!(shop.needs[0].identity.as_deref(), Some("export-data"));
    assert_eq!(shop.needs[0].customer_count, 2);
    assert_eq!(shop.needs[0].message_indices, vec![0, 1]);
    let other = report
        .products
        .iter()
        .find(|p| p.product_id == "other-shop")
        .unwrap();
    assert_eq!(other.needs[0].identity, None);
    Ok(())
}

#[test]
fn explicit_identity_correction_changes_future_analyses_without_rewriting_history()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    let mut source = Source(vec![feedback("first", Some("alice"))]);
    let classify = |_: &[Feedback]| {
        Ok(vec![Classification {
            message_index: 0,
            needs: vec![Need {
                name: "CSV export".into(),
                variant: None,
            }],
        }])
    };
    workflow.assign_need_identity("shop", "CSV export", "wrong")?;
    let before = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        classify,
    )?;
    assert!(
        workflow
            .correct_need_identity("shop", "CSV export", "outdated", "right")
            .is_err()
    );
    workflow.correct_need_identity("shop", "CSV export", "wrong", "right")?;
    let after = workflow.analyze(
        &mut source,
        "2026-11-13T00:00:00Z".parse()?,
        "2026-11-20T00:00:00Z".parse()?,
        classify,
    )?;
    assert_eq!(
        after.products[0].needs[0].identity.as_deref(),
        Some("right")
    );
    assert_eq!(workflow.report(before.id)?, before);
    Ok(())
}

#[test]
fn reviewed_report_recalculates_saved_classifications_after_mapping_without_rewriting_original()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    let mut source = Source(vec![
        feedback("first", Some("alice")),
        feedback("second", Some("bob")),
    ]);
    let original = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![Need {
                        name: "CSV export".into(),
                        variant: Some("CSV".into()),
                    }],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        name: "download CSV".into(),
                        variant: Some("CSV".into()),
                    }],
                },
            ])
        },
    )?;
    assert!(original.products[0].needs_identity_review);
    workflow.assign_need_identity("shop", "CSV export", "export")?;
    workflow.assign_need_identity("shop", "download CSV", "export")?;
    source.0.clear();
    let reviewed = workflow.review_report(original.id)?;
    assert_ne!(original.id, reviewed.id);
    assert!(!reviewed.products[0].needs_identity_review);
    assert_eq!(reviewed.products[0].needs.len(), 1);
    assert_eq!(reviewed.products[0].needs[0].customer_count, 2);
    assert_eq!(reviewed.input, original.input);
    assert_eq!(reviewed.classifications, original.classifications);
    assert_eq!(workflow.report(original.id)?, original);
    Ok(())
}

#[test]
fn review_queue_shows_unresolved_needs_with_evidence_and_confirmed_candidates() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite");
    let mut workflow = Workflow::open(&path)?;
    workflow.assign_need_identity("shop", "Export data", "export")?;
    workflow.assign_need_identity("other-shop", "Export data", "other-export")?;
    let mut first = feedback("1", Some("alice"));
    first.text = "Please let me download orders as CSV".into();
    let mut second = feedback("2", Some("alice"));
    second.text = "I need a CSV download".into();
    let mut source = Source(vec![first, second]);
    let report = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![Need {
                        name: "CSV download".into(),
                        variant: Some("CSV".into()),
                    }],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        name: "CSV download".into(),
                        variant: Some("CSV".into()),
                    }],
                },
            ])
        },
    )?;
    let queue = workflow.need_review_queue(report.id)?;
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].product_id, "shop");
    assert_eq!(queue[0].label, "CSV download");
    assert_eq!(queue[0].customer_count, 1);
    assert_eq!(
        queue[0].messages,
        vec![
            "Please let me download orders as CSV",
            "I need a CSV download"
        ]
    );
    assert_eq!(queue[0].variants, vec!["CSV"]);
    assert_eq!(queue[0].candidates.len(), 1);
    assert_eq!(queue[0].candidates[0].identity, "export");
    workflow.assign_need_identity("shop", "CSV download", "export")?;
    assert!(workflow.need_review_queue(report.id)?.is_empty());
    Ok(())
}

#[test]
fn interactive_review_confirms_suggestions_and_keeps_deferred_needs_unresolved()
-> anyhow::Result<()> {
    use ship_signal::{NeedSuggestion, SuggestionKind};
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    workflow.assign_need_identity("shop", "Export data", "export")?;
    let mut source = Source(vec![
        feedback("first", Some("alice")),
        feedback("second", Some("bob")),
    ]);
    let original = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![Need {
                        name: "CSV download".into(),
                        variant: Some("CSV".into()),
                    }],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        name: "Dark mode".into(),
                        variant: None,
                    }],
                },
            ])
        },
    )?;
    let mut input = std::io::Cursor::new(b"1\n4\n");
    let mut output = Vec::new();
    let reviewed = workflow
        .review_needs(original.id, &mut input, &mut output, false, |item| {
            assert_eq!(item.candidates[0].identity, "export");
            Ok(NeedSuggestion {
                kind: SuggestionKind::Existing,
                identity: Some("export".into()),
                reason: "Related export request".into(),
            })
        })?
        .unwrap();
    let output = String::from_utf8(output)?;
    assert!(output.contains("CSV download"));
    assert!(output.contains("Related export request"));
    assert!(output.contains("Dark mode"));
    assert!(reviewed.products[0].needs_identity_review);
    assert!(
        reviewed.products[0]
            .needs
            .iter()
            .any(|n| n.identity.as_deref() == Some("export"))
    );
    assert!(
        reviewed.products[0]
            .needs
            .iter()
            .any(|n| n.identity.is_none() && n.name == "Dark mode")
    );
    assert_eq!(workflow.report(original.id)?, original);
    Ok(())
}

#[test]
fn quitting_review_saves_only_confirmed_decisions_and_can_resume() -> anyhow::Result<()> {
    use ship_signal::{NeedSuggestion, SuggestionKind};
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    let mut source = Source(vec![
        feedback("first", Some("alice")),
        feedback("second", Some("bob")),
    ]);
    let original = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![Need {
                        name: "Alpha".into(),
                        variant: None,
                    }],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        name: "Beta".into(),
                        variant: None,
                    }],
                },
            ])
        },
    )?;
    let suggestion = |_: &ship_signal::NeedReviewItem| {
        Ok(NeedSuggestion {
            kind: SuggestionKind::New,
            identity: None,
            reason: "New need".into(),
        })
    };
    let result = workflow.review_needs(
        original.id,
        &mut std::io::Cursor::new(b"3\nalpha\nq\n"),
        &mut Vec::new(),
        false,
        suggestion,
    )?;
    assert!(result.is_none());
    assert_eq!(workflow.need_review_queue(original.id)?.len(), 1);
    let resumed = workflow
        .review_needs(
            original.id,
            &mut std::io::Cursor::new(b"2\nalpha\n"),
            &mut Vec::new(),
            false,
            |_| {
                Ok(NeedSuggestion {
                    kind: SuggestionKind::Uncertain,
                    identity: None,
                    reason: "Needs review".into(),
                })
            },
        )?
        .unwrap();
    assert!(!resumed.products[0].needs_identity_review);
    assert_eq!(resumed.products[0].needs[0].customer_count, 2);
    assert_eq!(workflow.report(original.id)?, original);
    Ok(())
}

#[test]
fn invalid_model_identity_cannot_write_a_mapping() -> anyhow::Result<()> {
    use ship_signal::{NeedSuggestion, SuggestionKind};
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    let mut source = Source(vec![feedback("first", Some("alice"))]);
    let original = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![Classification {
                message_index: 0,
                needs: vec![Need {
                    name: "Export".into(),
                    variant: None,
                }],
            }])
        },
    )?;
    let result = workflow.review_needs(
        original.id,
        &mut std::io::Cursor::new(b"1\n"),
        &mut Vec::new(),
        false,
        |_| {
            Ok(NeedSuggestion {
                kind: SuggestionKind::Existing,
                identity: Some("invented".into()),
                reason: "Match".into(),
            })
        },
    );
    assert!(result.is_err());
    assert!(workflow.need_identity_aliases()?.is_empty());
    assert_eq!(workflow.report(original.id)?, original);
    Ok(())
}

#[test]
fn deferred_decisions_survive_quit_and_can_be_explicitly_reopened() -> anyhow::Result<()> {
    use ship_signal::{NeedSuggestion, SuggestionKind};
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite");
    let mut workflow = Workflow::open(&path)?;
    let mut source = Source(vec![
        feedback("first", Some("alice")),
        feedback("second", Some("bob")),
    ]);
    let original = workflow.analyze(
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![Need {
                        name: "Alpha".into(),
                        variant: None,
                    }],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        name: "Beta".into(),
                        variant: None,
                    }],
                },
            ])
        },
    )?;
    let suggest = |_: &ship_signal::NeedReviewItem| {
        Ok(NeedSuggestion {
            kind: SuggestionKind::New,
            identity: None,
            reason: "New need".into(),
        })
    };
    assert!(
        workflow
            .review_needs(
                original.id,
                &mut std::io::Cursor::new(b"4\nq\n"),
                &mut Vec::new(),
                false,
                suggest
            )?
            .is_none()
    );
    drop(workflow);
    let mut workflow = Workflow::open(&path)?;
    let reviewed = workflow
        .review_needs(
            original.id,
            &mut std::io::Cursor::new(b"3\nbeta\n"),
            &mut Vec::new(),
            false,
            |item| {
                assert_eq!(item.label, "Beta");
                Ok(NeedSuggestion {
                    kind: SuggestionKind::New,
                    identity: None,
                    reason: "New need".into(),
                })
            },
        )?
        .unwrap();
    assert!(reviewed.products[0].needs_identity_review);
    let complete = workflow
        .review_needs(
            original.id,
            &mut std::io::Cursor::new(b"3\nalpha\n"),
            &mut Vec::new(),
            true,
            |item| {
                assert_eq!(item.label, "Alpha");
                Ok(NeedSuggestion {
                    kind: SuggestionKind::New,
                    identity: None,
                    reason: "New need".into(),
                })
            },
        )?
        .unwrap();
    assert!(!complete.products[0].needs_identity_review);
    Ok(())
}
