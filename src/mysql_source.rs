use crate::{
    AnalysisInput, FeedbackSource,
    feedback_row::{FEEDBACK_COLUMNS, FeedbackRow, validate_rows},
    validate_product_id,
};
use anyhow::{Context, Result, anyhow, ensure};
use chrono::{DateTime, NaiveDateTime, Utc};
use mysql::{
    AccessMode, Conn, Opts, TxOpts, Value, from_row_opt, from_value_opt, prelude::Queryable,
};

pub struct MysqlSource {
    client: Conn,
    view: String,
}

fn quote_identifier(value: &str) -> String {
    format!("`{}`", value.replace('`', "``"))
}

impl MysqlSource {
    pub fn connect(
        options: Opts,
        schema: Option<&str>,
        view: &str,
        timestamps_mapped_to_utc: bool,
    ) -> Result<Self> {
        ensure!(
            timestamps_mapped_to_utc,
            "explicitly confirm how the view maps source timestamps to UTC"
        );
        let schema = schema
            .or(options.get_db_name())
            .filter(|name| !name.is_empty())
            .context("specify a MySQL database in the URL or with --schema")?
            .to_owned();
        ensure!(!view.is_empty(), "view name must be nonempty");
        let mut client = Conn::new(options).context("connect to MySQL feedback database")?;
        client
            .query_drop("SET SESSION time_zone = '+00:00'")
            .context("set MySQL feedback session to UTC")?;
        client.query_drop("SET NAMES utf8mb4")?;

        let kind: Option<String> = client.exec_first(
            "SELECT TABLE_TYPE FROM information_schema.TABLES
             WHERE CAST(TABLE_SCHEMA AS BINARY) = CAST(? AS BINARY)
               AND CAST(TABLE_NAME AS BINARY) = CAST(? AS BINARY)",
            (&schema, view),
        )?;
        ensure!(
            kind.as_deref() == Some("VIEW"),
            "{schema}.{view} must be an existing MySQL view visible to the reader"
        );

        let qualified = format!("{}.{}", quote_identifier(&schema), quote_identifier(view));

        let columns: Vec<(String, String)> = client.exec(
            "SELECT COLUMN_NAME, DATA_TYPE FROM information_schema.COLUMNS
             WHERE CAST(TABLE_SCHEMA AS BINARY) = CAST(? AS BINARY)
               AND CAST(TABLE_NAME AS BINARY) = CAST(? AS BINARY)",
            (&schema, view),
        )?;
        for name in FEEDBACK_COLUMNS.split(", ") {
            let (_, data_type) = columns
                .iter()
                .find(|(column, _)| column.eq_ignore_ascii_case(name))
                .with_context(|| format!("view metadata is missing column {name}"))?;
            if name == "submitted_at" {
                ensure!(
                    matches!(data_type.as_str(), "datetime" | "timestamp"),
                    "view column {name} must have MySQL type DATETIME or TIMESTAMP (found {data_type}); map timestamps explicitly in the view"
                );
            } else {
                ensure!(
                    matches!(
                        data_type.as_str(),
                        "char" | "varchar" | "tinytext" | "text" | "mediumtext" | "longtext"
                    ),
                    "view column {name} must have a MySQL character string type (found {data_type}); cast identifiers and optional columns to text in the view"
                );
            }
        }
        client
            .query_drop(format!(
                "SELECT {FEEDBACK_COLUMNS} FROM {qualified} LIMIT 0"
            ))
            .context("view must expose the seven feedback contract columns and grant SELECT")?;

        Ok(Self {
            client,
            view: qualified,
        })
    }
}

impl FeedbackSource for MysqlSource {
    fn read(
        &mut self,
        product_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<AnalysisInput> {
        validate_product_id(product_id)?;
        ensure!(start < end, "interval start must precede end");
        let mut tx = self
            .client
            .start_transaction(TxOpts::default().set_access_mode(Some(AccessMode::ReadOnly)))?;
        // Compare UTF-8 bytes so connection and view collations cannot equate distinct IDs.
        let query = format!(
            "SELECT {FEEDBACK_COLUMNS} FROM {}
             WHERE CAST(CONVERT(product_id USING utf8mb4) AS BINARY) = CAST(? AS BINARY)
               AND ((submitted_at >= ? AND submitted_at < ?) OR submitted_at IS NULL
                    OR YEAR(submitted_at) = 0 OR MONTH(submitted_at) = 0 OR DAYOFMONTH(submitted_at) = 0
                    OR DAYOFMONTH(submitted_at) > DAYOFMONTH(LAST_DAY(submitted_at)))",
            self.view
        );
        let rows = tx
            .exec_iter(query, (product_id, start.naive_utc(), end.naive_utc()))
            .context("read MySQL feedback view")?;
        let input = validate_rows(rows.map(|row| {
            let (
                product_id,
                feedback_source_id,
                source_message_id,
                customer_id,
                submitted_at,
                text,
                source_reference,
            ): (_, _, _, _, Value, _, _) = from_row_opt(row.context("read MySQL feedback row")?)
                .map_err(|_| {
                    anyhow!("could not decode MySQL feedback row with the expected field types")
                })?;
            // MySQL can store zero or invalid calendar dates under permissive SQL modes.
            let submitted_at = from_value_opt::<Option<NaiveDateTime>>(submitted_at)
                .ok()
                .flatten()
                .map(|time| time.and_utc());
            Ok(FeedbackRow {
                product_id,
                feedback_source_id,
                source_message_id,
                customer_id,
                submitted_at,
                text,
                source_reference,
            })
        }))?;
        tx.commit()?;
        Ok(input)
    }
}
