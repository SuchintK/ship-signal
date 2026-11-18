use crate::{
    AnalysisInput, FeedbackSource,
    feedback_row::{FEEDBACK_COLUMNS, FeedbackRow, validate_rows},
    validate_product_id,
};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use native_tls::TlsConnector;
use postgres::{Client, types::Type};
use postgres_native_tls::MakeTlsConnector;

pub struct PostgresSource {
    client: Client,
    view: String,
}
fn quote_identifier(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
impl PostgresSource {
    pub fn connect(
        url: &str,
        schema: &str,
        view: &str,
        timestamps_mapped_to_utc: bool,
    ) -> Result<Self> {
        ensure!(
            timestamps_mapped_to_utc,
            "explicitly confirm how the view maps source timestamps to UTC"
        );
        ensure!(
            !schema.is_empty() && !view.is_empty(),
            "schema and view names must be nonempty"
        );
        let tls = MakeTlsConnector::new(TlsConnector::new()?);
        let mut client =
            Client::connect(url, tls).context("connect to PostgreSQL feedback database")?;
        let kind: Option<String> = client.query_opt("SELECT c.relkind::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2", &[&schema, &view])?.map(|r| r.get(0));
        ensure!(
            kind.as_deref() == Some("v"),
            "{schema}.{view} must be an existing PostgreSQL view"
        );
        let qualified = format!("{}.{}", quote_identifier(schema), quote_identifier(view));
        let stmt = client
            .prepare(&format!(
                "SELECT {FEEDBACK_COLUMNS} FROM {qualified} LIMIT 0"
            ))
            .context("view must expose the seven feedback contract columns and grant SELECT")?;
        let expected = [
            Type::TEXT,
            Type::TEXT,
            Type::TEXT,
            Type::TEXT,
            Type::TIMESTAMPTZ,
            Type::TEXT,
            Type::TEXT,
        ];
        for (column, expected) in stmt.columns().iter().zip(expected) {
            ensure!(
                *column.type_() == expected,
                "view column {} must have PostgreSQL type {} (found {}); cast identifiers and map timestamps explicitly in the view",
                column.name(),
                expected,
                column.type_()
            );
        }
        Ok(Self {
            client,
            view: qualified,
        })
    }
}
impl FeedbackSource for PostgresSource {
    fn read(
        &mut self,
        product_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<AnalysisInput> {
        validate_product_id(product_id)?;
        ensure!(start < end, "interval start must precede end");
        let mut tx = self.client.build_transaction().read_only(true).start()?;
        let query = format!(
            "SELECT {FEEDBACK_COLUMNS} FROM {} WHERE product_id = $1 AND ((submitted_at >= $2 AND submitted_at < $3) OR submitted_at IS NULL)",
            self.view
        );
        let rows = tx
            .query(&query, &[&product_id, &start, &end])
            .context("read feedback view")?;
        tx.commit()?;
        validate_rows(rows.into_iter().map(|row| {
            Ok(FeedbackRow {
                product_id: row.try_get(0)?,
                feedback_source_id: row.try_get(1)?,
                source_message_id: row.try_get(2)?,
                customer_id: row.try_get(3)?,
                submitted_at: row
                    .try_get(4)
                    .context("invalid or out-of-range timestamp in feedback view")?,
                text: row.try_get(5)?,
                source_reference: row.try_get(6)?,
            })
        }))
    }
}
