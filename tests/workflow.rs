use chrono::{DateTime, Utc};
use ship_signal::{AnalysisInput, Classification, Feedback, FeedbackSource, Need, Workflow};
use std::collections::BTreeMap;

struct Source(Vec<Feedback>);
impl FeedbackSource for Source {
    fn read(
        &mut self,
        product_id: &str,
        _: DateTime<Utc>,
        _: DateTime<Utc>,
    ) -> anyhow::Result<AnalysisInput> {
        Ok(AnalysisInput {
            feedback: self
                .0
                .iter()
                .filter(|row| row.product_id == product_id)
                .cloned()
                .collect(),
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
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![Classification {
                message_index: 0,
                needs: vec![Need {
                    need: "export".into(),
                    variant: Some("CSV".into()),
                }],
            }])
        },
    )?;
    source.0.clear();
    drop(workflow);
    let workflow = Workflow::open(&path)?;
    assert_eq!(workflow.report("shop", report.id)?, report);
    assert_eq!(report.productReport.denominator, 1);
    assert_eq!(report.productReport.needs[0].customer_count, 1);
    assert!(report.productReport.needs[0].qualifies);
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
        "shop",
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
                        .map(|(need_text, variant)| Need {
                            need: need_text.into(),
                            variant: variant.map(str::to_string),
                        })
                        .collect(),
                })
                .collect())
        },
    )?;
    let product = &report.productReport;
    assert_eq!(product.denominator, 4);
    let export = product.needs.iter().find(|n| n.need == "export").unwrap();
    assert_eq!(
        (export.customer_count, export.percentage, export.qualifies),
        (2, 50.0, true)
    );
    assert_eq!(export.variants, vec!["CSV", "PDF"]);
    assert_eq!(export.message_indices, vec![0, 1, 2, 6]);
    let dark = product
        .needs
        .iter()
        .find(|n| n.need == "dark mode")
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
        "shop",
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
                        need: "export".into(),
                        variant: None,
                    }],
                },
            ])
        },
    )?;
    assert_eq!(report.productReport.denominator, 0);
    assert!(!report.productReport.needs[0].qualifies);
    Ok(())
}

#[test]
fn confirmed_need_identity_connects_needs_across_reports_without_changing_old_evidence()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite");
    let mut workflow = Workflow::open(&path)?;
    let mut source = Source(vec![feedback("first", Some("alice"))]);
    let classify = |need_text: &'static str| {
        move |_: &[Feedback]| {
            Ok(vec![Classification {
                message_index: 0,
                needs: vec![Need {
                    need: need_text.into(),
                    variant: Some("CSV".into()),
                }],
            }])
        }
    };
    let old = workflow.analyze(
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        classify("CSV export"),
    )?;
    assert_eq!(old.productReport.needs[0].identity, None);
    workflow.assign_need_identity("shop", "CSV export", "export-data")?;
    workflow.assign_need_identity("shop", "download data as CSV", "export-data")?;
    drop(workflow);
    let mut workflow = Workflow::open(&path)?;
    let first = workflow.analyze(
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        classify("CSV export"),
    )?;
    let second = workflow.analyze(
        "shop",
        &mut source,
        "2026-11-13T00:00:00Z".parse()?,
        "2026-11-20T00:00:00Z".parse()?,
        classify("download data as CSV"),
    )?;
    assert_eq!(
        first.productReport.needs[0].identity.as_deref(),
        Some("export-data")
    );
    assert_eq!(
        second.productReport.needs[0].identity.as_deref(),
        Some("export-data")
    );
    assert_eq!(workflow.report("shop", old.id)?, old);
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
    workflow.assign_need_identity("other-shop", "Dark mode", "dark")?;
    let mut another_product = feedback("third", Some("alice"));
    another_product.product_id = "other-shop".into();
    let mut source = Source(vec![
        feedback("first", Some("alice")),
        feedback("second", Some("bob")),
        another_product,
    ]);
    let classify = |rows: &[Feedback]| {
        Ok(rows
            .iter()
            .enumerate()
            .map(|(message_index, row)| Classification {
                message_index,
                needs: vec![Need {
                    need: if row.source_message_id == "second" {
                        "download data as CSV"
                    } else {
                        "CSV export"
                    }
                    .into(),
                    variant: Some("CSV".into()),
                }],
            })
            .collect())
    };
    let shop = workflow.analyze(
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        classify,
    )?;
    let other = workflow.analyze(
        "other-shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        classify,
    )?;
    assert_eq!(shop.product_id, "shop");
    assert_eq!(shop.productReport.product_id, "shop");
    assert_eq!(shop.productReport.denominator, 2);
    assert_eq!(shop.productReport.needs.len(), 1);
    assert_eq!(
        shop.productReport.needs[0].identity.as_deref(),
        Some("export-data")
    );
    assert_eq!(shop.productReport.needs[0].customer_count, 2);
    assert_eq!(shop.productReport.needs[0].message_indices, vec![0, 1]);
    assert_eq!(other.product_id, "other-shop");
    assert_eq!(other.productReport.product_id, "other-shop");
    assert_eq!(other.productReport.denominator, 1);
    assert_eq!(other.productReport.needs[0].identity, None);
    assert_eq!(workflow.need_identity_aliases("shop")?.len(), 2);
    assert_eq!(workflow.need_identity_aliases("other-shop")?.len(), 1);
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
                need: "CSV export".into(),
                variant: None,
            }],
        }])
    };
    workflow.assign_need_identity("shop", "CSV export", "wrong")?;
    let before = workflow.analyze(
        "shop",
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
        "shop",
        &mut source,
        "2026-11-13T00:00:00Z".parse()?,
        "2026-11-20T00:00:00Z".parse()?,
        classify,
    )?;
    assert_eq!(
        after.productReport.needs[0].identity.as_deref(),
        Some("right")
    );
    assert_eq!(workflow.report("shop", before.id)?, before);
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
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![Need {
                        need: "CSV export".into(),
                        variant: Some("CSV".into()),
                    }],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        need: "download CSV".into(),
                        variant: Some("CSV".into()),
                    }],
                },
            ])
        },
    )?;
    assert!(original.productReport.needs_identity_review);
    workflow.assign_need_identity("shop", "CSV export", "export")?;
    workflow.assign_need_identity("shop", "download CSV", "export")?;
    source.0.clear();
    let reviewed = workflow.review_report("shop", original.id)?;
    assert_ne!(original.id, reviewed.id);
    assert!(!reviewed.productReport.needs_identity_review);
    assert_eq!(reviewed.productReport.needs.len(), 1);
    assert_eq!(reviewed.productReport.needs[0].customer_count, 2);
    assert_eq!(reviewed.input, original.input);
    assert_eq!(reviewed.classifications, original.classifications);
    assert_eq!(workflow.report("shop", original.id)?, original);
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
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![Need {
                        need: "CSV download".into(),
                        variant: Some("CSV".into()),
                    }],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        need: "CSV download".into(),
                        variant: Some("CSV".into()),
                    }],
                },
            ])
        },
    )?;
    let queue = workflow.need_review_queue("shop", report.id)?;
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].product_id, "shop");
    assert_eq!(queue[0].need, "CSV download");
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
    assert!(workflow.need_review_queue("shop", report.id)?.is_empty());
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
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![Need {
                        need: "CSV download".into(),
                        variant: Some("CSV".into()),
                    }],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        need: "Dark mode".into(),
                        variant: None,
                    }],
                },
            ])
        },
    )?;
    let mut input = std::io::Cursor::new(b"1\n4\n");
    let mut output = Vec::new();
    let reviewed = workflow
        .review_needs(
            "shop",
            original.id,
            &mut input,
            &mut output,
            false,
            |item| {
                assert_eq!(item.candidates[0].identity, "export");
                Ok(NeedSuggestion {
                    kind: SuggestionKind::Existing,
                    identity: Some("export".into()),
                    reason: "Related export request".into(),
                })
            },
        )?
        .unwrap();
    let output = String::from_utf8(output)?;
    assert!(output.contains("CSV download"));
    assert!(output.contains("Related export request"));
    assert!(output.contains("Dark mode"));
    assert!(reviewed.productReport.needs_identity_review);
    assert!(
        reviewed
            .productReport
            .needs
            .iter()
            .any(|n| n.identity.as_deref() == Some("export"))
    );
    assert!(
        reviewed
            .productReport
            .needs
            .iter()
            .any(|n| n.identity.is_none() && n.need == "Dark mode")
    );
    assert_eq!(workflow.report("shop", original.id)?, original);
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
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![Need {
                        need: "Alpha".into(),
                        variant: None,
                    }],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        need: "Beta".into(),
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
        "shop",
        original.id,
        &mut std::io::Cursor::new(b"3\nalpha\nq\n"),
        &mut Vec::new(),
        false,
        suggestion,
    )?;
    assert!(result.is_none());
    assert_eq!(workflow.need_review_queue("shop", original.id)?.len(), 1);
    let resumed = workflow
        .review_needs(
            "shop",
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
    assert!(!resumed.productReport.needs_identity_review);
    assert_eq!(resumed.productReport.needs[0].customer_count, 2);
    assert_eq!(workflow.report("shop", original.id)?, original);
    Ok(())
}

#[test]
fn invalid_model_identity_cannot_write_a_mapping() -> anyhow::Result<()> {
    use ship_signal::{NeedSuggestion, SuggestionKind};
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    let mut source = Source(vec![feedback("first", Some("alice"))]);
    let original = workflow.analyze(
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![Classification {
                message_index: 0,
                needs: vec![Need {
                    need: "Export".into(),
                    variant: None,
                }],
            }])
        },
    )?;
    let result = workflow.review_needs(
        "shop",
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
    assert!(workflow.need_identity_aliases("shop")?.is_empty());
    assert_eq!(workflow.report("shop", original.id)?, original);
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
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![
                Classification {
                    message_index: 0,
                    needs: vec![Need {
                        need: "Alpha".into(),
                        variant: None,
                    }],
                },
                Classification {
                    message_index: 1,
                    needs: vec![Need {
                        need: "Beta".into(),
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
                "shop",
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
            "shop",
            original.id,
            &mut std::io::Cursor::new(b"3\nbeta\n"),
            &mut Vec::new(),
            false,
            |item| {
                assert_eq!(item.need, "Beta");
                Ok(NeedSuggestion {
                    kind: SuggestionKind::New,
                    identity: None,
                    reason: "New need".into(),
                })
            },
        )?
        .unwrap();
    assert!(reviewed.productReport.needs_identity_review);
    let complete = workflow
        .review_needs(
            "shop",
            original.id,
            &mut std::io::Cursor::new(b"3\nalpha\n"),
            &mut Vec::new(),
            true,
            |item| {
                assert_eq!(item.need, "Alpha");
                Ok(NeedSuggestion {
                    kind: SuggestionKind::New,
                    identity: None,
                    reason: "New need".into(),
                })
            },
        )?
        .unwrap();
    assert!(!complete.productReport.needs_identity_review);
    Ok(())
}

#[test]
fn loads_reports_using_the_database_row_id() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite");
    // The saved JSON omits the ID; loading must use the requested SQLite row's ID.
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "CREATE TABLE analysis_reports (id INTEGER PRIMARY KEY, product_id TEXT NOT NULL, report TEXT NOT NULL);",
    )?;
    let report_content = r#"{"reviewed_from":null,"start":"2026-11-06T00:00:00Z","end":"2026-11-13T00:00:00Z","input":{"feedback":[],"diagnostics":{}},"classifications":[],"product":{"product_id":"shop","denominator":0,"needs_identity_review":false,"needs":[]}}"#;
    connection.execute(
        "INSERT INTO analysis_reports(id, product_id, report) VALUES (?1, 'shop', ?2)",
        rusqlite::params![41, report_content],
    )?;
    drop(connection);
    let workflow = Workflow::open(&path)?;
    assert_eq!(workflow.report("shop", 41)?.id, 41);
    // The original analysis has its own row ID and no review parent.
    assert!(workflow.report("shop", 41)?.reviewed_from.is_none());
    assert!(workflow.report("shop", 42).is_err());
    Ok(())
}

#[test]
fn saves_and_reopens_report_when_existing_reports_are_write_once() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite");
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch("CREATE TABLE analysis_reports (id INTEGER PRIMARY KEY, product_id TEXT NOT NULL, report TEXT NOT NULL); CREATE TRIGGER reports_are_write_once BEFORE UPDATE ON analysis_reports BEGIN SELECT RAISE(ABORT, 'reports are write once'); END;")?;
    drop(connection);
    let mut workflow = Workflow::open(&path)?;
    let mut source = Source(vec![feedback("1", Some("alice"))]);
    let report = workflow.analyze(
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![Classification {
                message_index: 0,
                needs: vec![Need {
                    need: "export".into(),
                    variant: None,
                }],
            }])
        },
    )?;
    assert!(report.id > 0);
    drop(workflow);
    assert_eq!(Workflow::open(&path)?.report("shop", report.id)?, report);
    Ok(())
}

#[test]
fn reports_and_review_inputs_use_need_consistently() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    workflow.assign_need_identity("shop", "Export orders", "export-orders")?;
    let report = workflow.analyze(
        "shop",
        &mut Source(vec![feedback("request", Some("alice"))]),
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![Classification {
                message_index: 0,
                needs: vec![Need {
                    need: "Download orders".into(),
                    variant: Some("CSV".into()),
                }],
            }])
        },
    )?;
    let json = serde_json::to_value(&report)?;
    assert_eq!(
        json["classifications"][0]["needs"][0]["need"],
        "Download orders"
    );
    assert!(json["classifications"][0]["needs"][0].get("name").is_none());
    assert_eq!(json["product"]["needs"][0]["need"], "Download orders");
    let queue = serde_json::to_value(workflow.need_review_queue("shop", report.id)?)?;
    assert_eq!(queue[0]["need"], "Download orders");
    assert_eq!(
        queue[0]["candidates"][0]["needs"],
        serde_json::json!(["Export orders"])
    );
    let mappings = serde_json::to_value(workflow.need_identity_aliases("shop")?)?;
    assert_eq!(mappings[0]["need"], "Export orders");
    assert!(mappings[0].get("label").is_none());
    Ok(())
}

#[test]
fn empty_reports_keep_product_ownership_and_reject_cross_product_operations() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite");
    let mut workflow = Workflow::open(&path)?;
    let original = workflow.analyze(
        "shop",
        &mut Source(vec![]),
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |rows| {
            assert!(rows.is_empty());
            Ok(vec![])
        },
    )?;
    assert_eq!(original.product_id, "shop");
    assert_eq!(original.productReport.product_id, "shop");
    assert_eq!(original.productReport.denominator, 0);
    assert!(original.productReport.needs.is_empty());
    assert!(!original.productReport.needs_identity_review);
    assert!(workflow.report("other-shop", original.id).is_err());
    assert!(workflow.review_report("other-shop", original.id).is_err());
    assert!(
        workflow
            .need_review_queue("other-shop", original.id)
            .is_err()
    );
    let result = workflow.review_needs(
        "other-shop",
        original.id,
        &mut std::io::Cursor::new(b"3\nwrong\n"),
        &mut Vec::new(),
        true,
        |_| panic!("a foreign report must not reach the model"),
    );
    assert!(result.is_err());
    assert!(workflow.need_identity_aliases("other-shop")?.is_empty());
    drop(workflow);
    let mut workflow = Workflow::open(&path)?;
    assert_eq!(workflow.report("shop", original.id)?, original);
    let reviewed = workflow.review_report("shop", original.id)?;
    assert_eq!(reviewed.product_id, "shop");
    // The review links back to the input report for the same product.
    assert_eq!(reviewed.reviewed_from, Some(original.id));
    assert_eq!(reviewed.productReport, original.productReport);
    Ok(())
}

#[test]
fn a_foreign_source_row_is_rejected_before_classification_or_saving() -> anyhow::Result<()> {
    struct BrokenSource;
    impl FeedbackSource for BrokenSource {
        fn read(
            &mut self,
            _: &str,
            _: DateTime<Utc>,
            _: DateTime<Utc>,
        ) -> anyhow::Result<AnalysisInput> {
            let mut foreign = feedback("foreign", Some("mallory"));
            foreign.product_id = "other-shop".into();
            Ok(AnalysisInput {
                feedback: vec![foreign],
                diagnostics: BTreeMap::new(),
            })
        }
    }
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    let result = workflow.analyze(
        "shop",
        &mut BrokenSource,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| panic!("foreign feedback must not reach classification"),
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("outside the requested product")
    );
    assert!(workflow.report("shop", 1).is_err());
    assert!(
        workflow
            .analyze(
                "   ",
                &mut BrokenSource,
                "2026-11-06T00:00:00Z".parse()?,
                "2026-11-13T00:00:00Z".parse()?,
                |_| panic!("blank product must be rejected")
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn need_identity_matching_ignores_case_and_preserves_classifications() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite");
    let mut workflow = Workflow::open(&path)?;
    let mut source = Source(vec![
        feedback("one", Some("alice")),
        feedback("two", Some("bob")),
    ]);
    let classifications = vec![
        Classification {
            message_index: 0,
            needs: vec![Need {
                need: "Export orders".into(),
                variant: Some("CSV".into()),
            }],
        },
        Classification {
            message_index: 1,
            needs: vec![Need {
                need: "EXPORT ORDERS".into(),
                variant: Some("PDF".into()),
            }],
        },
    ];
    let original = workflow.analyze(
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| Ok(classifications.clone()),
    )?;
    workflow.assign_need_identity("shop", "export orders", "export-orders")?;
    assert!(workflow.need_review_queue("shop", original.id)?.is_empty());
    let reviewed = workflow.review_report("shop", original.id)?;
    assert_eq!(reviewed.classifications, classifications);
    assert_eq!(reviewed.productReport.needs.len(), 1);
    assert_eq!(
        reviewed.productReport.needs[0].identity.as_deref(),
        Some("export-orders")
    );
    assert_eq!(reviewed.productReport.needs[0].customer_count, 2);
    assert_eq!(reviewed.productReport.needs[0].variants, ["CSV", "PDF"]);
    assert!(!reviewed.productReport.needs_identity_review);
    assert_eq!(workflow.report("shop", original.id)?, original);
    drop(workflow);
    let mut workflow = Workflow::open(&path)?;
    let analyzed = workflow.analyze("shop", &mut source, original.start, original.end, |_| {
        Ok(classifications.clone())
    })?;
    assert_eq!(analyzed.productReport, reviewed.productReport);
    assert_eq!(analyzed.classifications, classifications);
    Ok(())
}

#[test]
fn case_insensitive_mappings_are_guarded_and_product_scoped() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    workflow.assign_need_identity("shop", "Éditer les commandes", "edit-orders")?;
    workflow.assign_need_identity("shop", "ÉDITER LES COMMANDES", "edit-orders")?;
    assert_eq!(workflow.need_identity_aliases("shop")?.len(), 1);
    assert!(
        workflow
            .assign_need_identity("shop", "éditer les commandes", "conflict")
            .is_err()
    );
    assert!(
        workflow
            .correct_need_identity("shop", "éditer les commandes", "outdated", "corrected")
            .is_err()
    );
    workflow.correct_need_identity("shop", "éditer les commandes", "edit-orders", "corrected")?;
    assert_eq!(
        workflow.need_identity_aliases("shop")?[0].identity,
        "corrected"
    );
    assert_eq!(
        workflow.need_identity_aliases("shop")?[0].need,
        "Éditer les commandes"
    );
    workflow.assign_need_identity("Shop", "ÉDITER LES COMMANDES", "other-product")?;
    let mut row = feedback("one", Some("alice"));
    row.text = "ÉDITER LES COMMANDES".into();
    let mut source = Source(vec![row]);
    let report = workflow.analyze(
        "shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![Classification {
                message_index: 0,
                needs: vec![Need {
                    need: "ÉDITER LES COMMANDES".into(),
                    variant: None,
                }],
            }])
        },
    )?;
    assert_eq!(
        report.productReport.needs[0].identity.as_deref(),
        Some("corrected")
    );
    assert_eq!(
        workflow.need_identity_aliases("Shop")?[0].identity,
        "other-product"
    );
    Ok(())
}
