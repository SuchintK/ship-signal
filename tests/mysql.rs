use ship_signal::{
    Classification, FeedbackSource, Need, Workflow, mysql_source::MysqlSource,
    postgres_source::PostgresSource,
};
use std::process::Command;

fn mysql_options() -> anyhow::Result<mysql::Opts> {
    Ok(mysql::Opts::from_url(&std::env::var(
        "SHIP_SIGNAL_TEST_MYSQL",
    )?)?)
}

#[test]
#[ignore = "requires SHIP_SIGNAL_TEST_MYSQL and the MySQL fixture"]
fn validates_mysql_views_and_reads_explicit_utc_with_a_select_only_account() -> anyhow::Result<()> {
    use mysql::prelude::Queryable;

    let options = mysql_options()?;
    assert!(MysqlSource::connect(options.clone(), None, "ship_signal_feedback", false).is_err());
    for (view, diagnostic) in [
        ("customer_feedback", "must be an existing MySQL view"),
        ("missing_column", "feedback_source_id"),
        ("wrong_type", "customer_id"),
        ("no_select", "must be an existing MySQL view"),
    ] {
        let Err(error) = MysqlSource::connect(options.clone(), None, view, true) else {
            panic!("accepted misconfigured view {view}");
        };
        assert!(format!("{error:#}").contains(diagnostic), "{error:#}");
    }

    let mut reader = mysql::Conn::new(options.clone())?;
    let lower_case_names: u8 = reader
        .query_first("SELECT @@lower_case_table_names")?
        .unwrap();
    let uppercase_schema = options.get_db_name().unwrap().to_ascii_uppercase();
    assert_eq!(
        MysqlSource::connect(
            options.clone(),
            Some(&uppercase_schema),
            "SHIP_SIGNAL_FEEDBACK",
            true
        )
        .is_ok(),
        lower_case_names != 0
    );
    assert!(
        reader
            .query_drop("UPDATE customer_feedback SET message = 'changed' WHERE id = 1")
            .is_err()
    );
    assert!(
        reader
            .query_drop("UPDATE ship_signal_feedback SET feedback_text = 'changed' WHERE source_message_id = '1'")
            .is_err()
    );

    for view in [
        "ship_signal_local_time_feedback",
        "ship_signal_timestamp_feedback",
    ] {
        let mut source = MysqlSource::connect(options.clone(), None, view, true)?;
        let input = source.read(
            "shop",
            "2026-11-07T03:30:00.123456Z".parse()?,
            "2026-11-07T03:30:00.123457Z".parse()?,
        )?;
        assert_eq!(input.feedback.len(), 1);
        assert_eq!(
            input.feedback[0].submitted_at.to_rfc3339(),
            "2026-11-07T03:30:00.123456+00:00"
        );
        assert_eq!(input.feedback[0].customer_id, None);
        assert_eq!(
            input.feedback[0].source_reference.as_deref(),
            Some("support/42")
        );
    }
    let mut source = MysqlSource::connect(options, None, "legacy_dates", true)?;
    let input = source.read(
        "shop",
        "2026-11-07T00:00:00Z".parse()?,
        "2026-11-08T00:00:00Z".parse()?,
    )?;
    assert!(input.feedback.is_empty());
    assert_eq!(
        input.diagnostics,
        [("invalid_rows".into(), 4), ("submitted_at".into(), 4)].into()
    );
    Ok(())
}

#[test]
#[ignore = "requires SHIP_SIGNAL_TEST_MYSQL and the MySQL fixture"]
fn mysql_filters_exact_product_before_validating_rows_and_interval_boundaries() -> anyhow::Result<()>
{
    let options = mysql_options()?;
    let mut source = MysqlSource::connect(options.clone(), None, "ship_signal_feedback", true)?;
    let input = source.read(
        "demo-shop",
        "2026-11-06T10:00:00Z".parse()?,
        "2026-11-10T14:00:00Z".parse()?,
    )?;
    assert_eq!(
        input
            .feedback
            .iter()
            .map(|row| row.source_message_id.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3", "4"]
    );
    assert!(input.diagnostics.is_empty());

    let mut source =
        MysqlSource::connect(options, None, "ship_signal_product_scoped_feedback", true)?;
    let start = "2026-11-06T00:00:00Z".parse()?;
    let end = "2026-11-13T00:00:00Z".parse()?;
    let input = source.read("shop", start, end)?;
    assert_eq!(input.feedback.len(), 1);
    assert_eq!(input.feedback[0].text, "Please add CSV.");
    assert_eq!(
        input.diagnostics,
        [("invalid_rows".into(), 1), ("submitted_at".into(), 1)].into()
    );
    let error = source
        .read("shop", "0999-01-01T00:00:00Z".parse()?, end)
        .unwrap_err();
    assert!(error.to_string().contains("1000"));
    let other = source.read("other-shop", start, end)?;
    assert_eq!(other.feedback.len(), 1);
    assert_eq!(other.feedback[0].text, "FOREIGN_PRODUCT_MESSAGE");
    assert_eq!(other.diagnostics.get("invalid_rows"), Some(&2));
    for variant in ["SHOP", "shop ", "shóp"] {
        let separate = source.read(variant, start, end)?;
        assert!(separate.feedback.is_empty());
        assert_eq!(
            separate.diagnostics.get("feedback_text"),
            Some(&1),
            "selected product {variant:?}"
        );
    }
    assert!(source.read("   ", start, end).is_err());
    Ok(())
}

#[test]
#[ignore = "requires SHIP_SIGNAL_TEST_MYSQL and SHIP_SIGNAL_TEST_POSTGRES and both fixtures"]
fn mysql_and_postgres_save_equivalent_reports_with_controlled_classifications() -> anyhow::Result<()>
{
    let mysql = mysql_options()?;
    let postgres = std::env::var("SHIP_SIGNAL_TEST_POSTGRES")?;
    let mut mysql_source = MysqlSource::connect(mysql.clone(), None, "ship_signal_feedback", true)?;
    let mut pg_source = PostgresSource::connect(&postgres, "public", "ship_signal_feedback", true)?;
    for source in [&mut mysql_source as &mut dyn FeedbackSource, &mut pg_source] {
        let before = "2026-11-06T09:59:59.999999999Z".parse()?;
        let at = "2026-11-06T10:00:00Z".parse()?;
        let after = "2026-11-06T10:00:00.000000001Z".parse()?;
        let next = "2026-11-06T10:00:00.000001Z".parse()?;
        assert_eq!(source.read("demo-shop", before, after)?.feedback.len(), 1);
        assert!(source.read("demo-shop", before, at)?.feedback.is_empty());
        assert!(source.read("demo-shop", after, next)?.feedback.is_empty());
    }

    let dir = tempfile::tempdir()?;
    let mysql_state = dir.path().join("mysql.sqlite");
    let pg_state = dir.path().join("postgres.sqlite");
    let mut mysql_workflow = Workflow::open(&mysql_state)?;
    let mut pg_workflow = Workflow::open(&pg_state)?;
    for workflow in [&mysql_workflow, &pg_workflow] {
        workflow.assign_need_identity("demo-shop", "ÉXPORT ORDERS", "export-orders")?;
        workflow.assign_need_identity("shop", "éxport orders", "shop-export")?;
    }

    let start = "2026-11-06T00:00:00Z".parse()?;
    let end = "2026-11-13T00:00:00Z".parse()?;
    for (view, product) in [
        ("ship_signal_feedback", "demo-shop"),
        ("ship_signal_invalid_feedback", "shop"),
        ("ship_signal_product_scoped_feedback", "shop"),
        ("ship_signal_feedback", "absent"),
    ] {
        let mut mysql = MysqlSource::connect(mysql.clone(), None, view, true)?;
        let mut postgres = PostgresSource::connect(&postgres, "public", view, true)?;
        let classify = |rows: &[ship_signal::Feedback]| {
            Ok(rows
                .iter()
                .enumerate()
                .map(|(message_index, row)| {
                    assert_eq!(row.product_id, product);
                    let (need, variant) = match row.source_message_id.as_str() {
                        "1" | "2" | "5" | "valid" | "shared" => ("éxport orders", Some("CSV")),
                        "3" => ("Éxport orders", Some("PDF")),
                        "4" => ("Dark mode", None),
                        _ => {
                            return Classification {
                                message_index,
                                needs: vec![],
                            };
                        }
                    };
                    Classification {
                        message_index,
                        needs: vec![Need {
                            need: need.into(),
                            variant: variant.map(String::from),
                        }],
                    }
                })
                .collect())
        };
        let report = mysql_workflow.analyze(product, &mut mysql, start, end, classify)?;
        assert_eq!(
            report,
            pg_workflow.analyze(product, &mut postgres, start, end, classify)?
        );
        assert_eq!(
            report,
            Workflow::open(&mysql_state)?.report(product, report.id)?
        );
        assert!(mysql_workflow.report("another-product", report.id).is_err());
        match view {
            "ship_signal_invalid_feedback" => {
                assert_eq!(report.input.diagnostics.get("invalid_rows"), Some(&4));
                assert_eq!(
                    report.input.diagnostics.get("duplicate_message_identity"),
                    Some(&2)
                );
            }
            _ if product == "demo-shop" => {
                let export = report
                    .product
                    .needs
                    .iter()
                    .find(|need| need.identity.as_deref() == Some("export-orders"))
                    .unwrap();
                assert_eq!(report.product.denominator, 3);
                assert_eq!(export.customer_count, 2);
                assert_eq!(export.variants, ["CSV", "PDF"]);
                assert_eq!(export.message_indices, [0, 1, 2, 4]);
                assert!(export.qualifies);
                assert_eq!(report.classifications[2].needs[0].need, "Éxport orders");
            }
            _ if product == "absent" => {
                assert_eq!(report.product.product_id, "absent");
                assert_eq!(report.product.denominator, 0);
                assert!(report.product.needs.is_empty());
            }
            _ => {}
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires SHIP_SIGNAL_TEST_MYSQL and the MySQL fixture"]
fn cli_validates_and_saves_a_mysql_report_for_an_absent_product() -> anyhow::Result<()> {
    let url = std::env::var("SHIP_SIGNAL_TEST_MYSQL")?;
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("cli.sqlite");
    for command in ["validate", "analyze"] {
        let mut cli = Command::new(env!("CARGO_BIN_EXE_ship-signal"));
        cli.arg(command)
            .env("FEEDBACK_DATABASE_URL", &url)
            .env_remove("OPENAI_API_KEY")
            .args([
                "--product",
                "absent",
                "--timestamps-mapped-to-utc",
                "--mysql-no-tls",
            ]);
        if command == "analyze" {
            cli.args([
                "--start",
                "2026-11-06T00:00:00Z",
                "--end",
                "2026-11-13T00:00:00Z",
                "--model",
                "unused",
                "--state",
            ])
            .arg(&state);
        }
        let result = cli.output()?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let report = Workflow::open(state)?.report("absent", 1)?;
    assert_eq!(report.product_id, "absent");
    assert!(report.input.feedback.is_empty());
    assert!(report.input.diagnostics.is_empty());
    Ok(())
}
