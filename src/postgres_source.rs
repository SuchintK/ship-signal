use crate::{AnalysisInput, Feedback, FeedbackSource};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use native_tls::TlsConnector;
use postgres::{Client, types::Type};
use postgres_native_tls::MakeTlsConnector;
use std::collections::{BTreeMap, BTreeSet};

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
        let stmt = client.prepare(&format!("SELECT product_id, feedback_source_id, source_message_id, customer_id, submitted_at, feedback_text, source_reference FROM {qualified} LIMIT 0")).context("view must expose the seven feedback contract columns and grant SELECT")?;
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
    fn read(&mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<AnalysisInput> {
        ensure!(start < end, "interval start must precede end");
        let mut tx = self.client.build_transaction().read_only(true).start()?;
        let query = format!(
            "SELECT product_id, feedback_source_id, source_message_id, customer_id, submitted_at, feedback_text, source_reference FROM {} WHERE (submitted_at >= $1 AND submitted_at < $2) OR submitted_at IS NULL ORDER BY submitted_at NULLS LAST, product_id, feedback_source_id, source_message_id",
            self.view
        );
        let rows = tx
            .query(&query, &[&start, &end])
            .context("read feedback view")?;
        tx.commit()?;
        let mut diagnostics = BTreeMap::new();
        let mut candidates = Vec::new();
        for row in rows {
            let product_id: Option<String> = row.try_get(0)?;
            let source_id: Option<String> = row.try_get(1)?;
            let message_id: Option<String> = row.try_get(2)?;
            let customer_id: Option<String> = row.try_get(3)?;
            let submitted_at: Option<DateTime<Utc>> = row
                .try_get(4)
                .context("invalid or out-of-range timestamp in feedback view")?;
            let text: Option<String> = row.try_get(5)?;
            let source_reference: Option<String> = row.try_get(6)?;
            let mut reasons = Vec::new();
            for (name, value) in [
                ("product_id", &product_id),
                ("feedback_source_id", &source_id),
                ("source_message_id", &message_id),
                ("feedback_text", &text),
            ] {
                if value.as_ref().is_none_or(|v| v.trim().is_empty()) {
                    reasons.push(name);
                }
            }
            if customer_id.as_ref().is_some_and(|v| v.trim().is_empty()) {
                reasons.push("customer_id");
            }
            if submitted_at.is_none() {
                reasons.push("submitted_at");
            }
            if !reasons.is_empty() {
                *diagnostics.entry("invalid_rows".into()).or_insert(0) += 1;
                for reason in reasons {
                    *diagnostics.entry(reason.into()).or_insert(0) += 1;
                }
                continue;
            }
            candidates.push(Feedback {
                product_id: product_id.unwrap(),
                feedback_source_id: source_id.unwrap(),
                source_message_id: message_id.unwrap(),
                customer_id,
                submitted_at: submitted_at.unwrap(),
                text: text.unwrap(),
                source_reference,
            });
        }
        let mut seen = BTreeSet::new();
        let mut duplicates = BTreeSet::new();
        for row in &candidates {
            let key = (
                row.product_id.clone(),
                row.feedback_source_id.clone(),
                row.source_message_id.clone(),
            );
            if !seen.insert(key.clone()) {
                duplicates.insert(key);
            }
        }
        candidates.retain(|row| {
            let duplicate = duplicates.contains(&(
                row.product_id.clone(),
                row.feedback_source_id.clone(),
                row.source_message_id.clone(),
            ));
            if duplicate {
                *diagnostics.entry("invalid_rows".into()).or_insert(0) += 1;
                *diagnostics
                    .entry("duplicate_message_identity".into())
                    .or_insert(0) += 1;
            }
            !duplicate
        });
        Ok(AnalysisInput {
            feedback: candidates,
            diagnostics,
        })
    }
}
