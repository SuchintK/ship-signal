use ship_signal::{FeedbackSource, postgres_source::PostgresSource};

#[test]
#[ignore = "requires SHIP_SIGNAL_TEST_POSTGRES and the PostgreSQL fixtures"]
fn reads_real_view_at_half_open_boundaries_without_writing() -> anyhow::Result<()> {
    let url = std::env::var("SHIP_SIGNAL_TEST_POSTGRES")?;
    let mut source = PostgresSource::connect(&url, "public", "ship_signal_feedback", true)?;
    let input = source.read(
        "demo-shop",
        "2026-11-06T10:00:00Z".parse()?,
        "2026-11-10T14:00:00Z".parse()?,
    )?;
    assert_eq!(input.feedback.len(), 4);
    assert_eq!(input.feedback[0].source_message_id, "1");
    assert_eq!(input.feedback[1].customer_id.as_deref(), Some("alice"));
    assert!(input.diagnostics.is_empty());
    assert!(PostgresSource::connect(&url, "public", "ship_signal_feedback", false).is_err());
    assert!(PostgresSource::connect(&url, "public", "customer_feedback", true).is_err());
    Ok(())
}

#[test]
#[ignore = "requires SHIP_SIGNAL_TEST_POSTGRES and the PostgreSQL fixtures"]
fn reports_invalid_rows_and_excludes_ambiguous_message_identities() -> anyhow::Result<()> {
    let url = std::env::var("SHIP_SIGNAL_TEST_POSTGRES")?;
    let mut source = PostgresSource::connect(&url, "public", "ship_signal_invalid_feedback", true)?;
    let input = source.read(
        "shop",
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
    )?;
    assert_eq!(input.feedback.len(), 1);
    assert_eq!(input.feedback[0].source_message_id, "valid");
    assert_eq!(input.diagnostics.get("invalid_rows"), Some(&4));
    assert_eq!(
        input.diagnostics.get("duplicate_message_identity"),
        Some(&2)
    );
    assert_eq!(input.diagnostics.get("feedback_text"), Some(&1));
    assert_eq!(input.diagnostics.get("submitted_at"), Some(&1));
    Ok(())
}

#[test]
#[ignore = "requires SHIP_SIGNAL_TEST_POSTGRES and the PostgreSQL fixtures"]
fn workflow_saves_a_report_from_real_postgresql() -> anyhow::Result<()> {
    use ship_signal::{Classification, Need, Workflow};
    let url = std::env::var("SHIP_SIGNAL_TEST_POSTGRES")?;
    let dir = tempfile::tempdir()?;
    let mut source = PostgresSource::connect(&url, "public", "ship_signal_feedback", true)?;
    let mut workflow = Workflow::open(dir.path().join("state.sqlite"))?;
    let report = workflow.analyze(
        "demo-shop",
        &mut source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |rows| {
            Ok(rows
                .iter()
                .enumerate()
                .map(|(message_index, row)| Classification {
                    message_index,
                    needs: match row.source_message_id.as_str() {
                        "1" | "2" | "5" => vec![Need {
                            need: "export".into(),
                            variant: Some("CSV".into()),
                        }],
                        "3" => vec![Need {
                            need: "export".into(),
                            variant: Some("PDF".into()),
                        }],
                        "4" => vec![Need {
                            need: "dark mode".into(),
                            variant: None,
                        }],
                        _ => vec![],
                    },
                })
                .collect())
        },
    )?;
    assert_eq!(report.product.denominator, 3);
    let export = report
        .product
        .needs
        .iter()
        .find(|need| need.need == "export")
        .unwrap();
    assert_eq!(export.customer_count, 2);
    assert_eq!(export.message_indices.len(), 4);
    assert_eq!(workflow.report("demo-shop", report.id)?, report);
    Ok(())
}

#[test]
#[ignore = "requires SHIP_SIGNAL_TEST_POSTGRES and the PostgreSQL fixtures"]
fn maps_non_utc_source_time_and_uses_read_only_account() -> anyhow::Result<()> {
    let url = std::env::var("SHIP_SIGNAL_TEST_POSTGRES")?;
    let mut source =
        PostgresSource::connect(&url, "public", "ship_signal_local_time_feedback", true)?;
    let input = source.read(
        "shop",
        "2026-11-07T03:30:00Z".parse()?,
        "2026-11-07T03:30:01Z".parse()?,
    )?;
    assert_eq!(input.feedback.len(), 1);
    assert_eq!(
        input.feedback[0].submitted_at.to_rfc3339(),
        "2026-11-07T03:30:00+00:00"
    );
    let mut client = postgres::Client::connect(&url, postgres::NoTls)?;
    let row = client.query_one(
        "SELECT has_table_privilege(current_user, 'customer_feedback', 'INSERT,UPDATE,DELETE')",
        &[],
    )?;
    assert!(
        !row.get::<_, bool>(0),
        "fixture account must not have write privileges"
    );
    Ok(())
}

#[test]
#[ignore = "requires SHIP_SIGNAL_TEST_POSTGRES and the PostgreSQL fixtures"]
fn product_filter_is_applied_before_capture_and_diagnostics() -> anyhow::Result<()> {
    let url = std::env::var("SHIP_SIGNAL_TEST_POSTGRES")?;
    let mut source =
        PostgresSource::connect(&url, "public", "ship_signal_product_scoped_feedback", true)?;
    let start = "2026-11-06T00:00:00Z".parse()?;
    let end = "2026-11-13T00:00:00Z".parse()?;
    let input = source.read("shop", start, end)?;
    assert_eq!(input.feedback.len(), 1);
    assert_eq!(input.feedback[0].product_id, "shop");
    assert_eq!(input.feedback[0].source_message_id, "shared");
    assert_eq!(input.diagnostics.get("invalid_rows"), Some(&1));
    assert_eq!(input.diagnostics.get("submitted_at"), Some(&1));
    assert!(!input.diagnostics.contains_key("duplicate_message_identity"));
    assert!(!input.diagnostics.contains_key("feedback_text"));
    let other = source.read("other-shop", start, end)?;
    assert_eq!(other.feedback.len(), 1);
    assert_eq!(other.feedback[0].text, "FOREIGN_PRODUCT_MESSAGE");
    assert_eq!(other.diagnostics.get("invalid_rows"), Some(&2));
    let absent = source.read("absent", start, end)?;
    assert!(absent.feedback.is_empty() && absent.diagnostics.is_empty());
    assert!(source.read("   ", start, end).is_err());
    Ok(())
}
