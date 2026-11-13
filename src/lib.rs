use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub mod postgres_source;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Feedback {
    pub product_id: String,
    pub feedback_source_id: String,
    pub source_message_id: String,
    pub customer_id: Option<String>,
    pub submitted_at: DateTime<Utc>,
    pub text: String,
    pub source_reference: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Need {
    pub name: String,
    pub variant: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Classification {
    pub message_index: usize,
    pub needs: Vec<Need>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Classifications {
    pub classifications: Vec<Classification>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalysisInput {
    pub feedback: Vec<Feedback>,
    pub diagnostics: BTreeMap<String, usize>,
}
pub trait FeedbackSource {
    fn read(&mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<AnalysisInput>;
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeedReport {
    pub name: String,
    pub customer_count: usize,
    pub percentage: f64,
    pub qualifies: bool,
    pub variants: Vec<String>,
    pub message_indices: Vec<usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProductReport {
    pub product_id: String,
    pub denominator: usize,
    pub needs: Vec<NeedReport>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Report {
    pub id: i64,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub input: AnalysisInput,
    pub classifications: Vec<Classification>,
    pub products: Vec<ProductReport>,
}

#[derive(Default)]
struct ProductDemand {
    customers: BTreeSet<String>,
    needs: BTreeMap<String, NeedDemand>,
}
#[derive(Default)]
struct NeedDemand {
    customers: BTreeSet<String>,
    variants: BTreeSet<String>,
    messages: BTreeSet<usize>,
}

pub struct Workflow {
    connection: Connection,
}
impl Workflow {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS analysis_reports (id INTEGER PRIMARY KEY, report TEXT NOT NULL);")?;
        Ok(Self { connection })
    }
    pub fn analyze(
        &mut self,
        source: &mut impl FeedbackSource,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        classify: impl FnOnce(&[Feedback]) -> Result<Vec<Classification>>,
    ) -> Result<Report> {
        ensure!(start < end, "interval start must precede end");
        let input = source.read(start, end)?;
        let mut classifications = classify(&input.feedback)?;
        classifications.sort_by_key(|c| c.message_index);
        ensure!(
            classifications.len() == input.feedback.len(),
            "classifier must return one classification per message"
        );
        for (i, classification) in classifications.iter().enumerate() {
            ensure!(
                classification.message_index == i,
                "classifier returned missing, duplicate, or invalid message index"
            );
            for need in &classification.needs {
                ensure!(
                    !need.name.trim().is_empty(),
                    "classifier returned an empty need name"
                );
                ensure!(
                    need.variant.as_ref().is_none_or(|v| !v.trim().is_empty()),
                    "classifier returned an empty variant"
                );
            }
        }
        let mut products: BTreeMap<String, ProductDemand> = BTreeMap::new();
        for (row, classification) in input.feedback.iter().zip(&classifications) {
            let product = products.entry(row.product_id.clone()).or_default();
            if !classification.needs.is_empty() {
                if let Some(customer) = &row.customer_id {
                    product.customers.insert(customer.clone());
                }
            }
            for need in &classification.needs {
                let group = product.needs.entry(need.name.clone()).or_default();
                if let Some(customer) = &row.customer_id {
                    group.customers.insert(customer.clone());
                }
                if let Some(variant) = &need.variant {
                    group.variants.insert(variant.clone());
                }
                group.messages.insert(classification.message_index);
            }
        }
        let products = products
            .into_iter()
            .map(|(product_id, product)| {
                let denominator = product.customers.len();
                let needs = product
                    .needs
                    .into_iter()
                    .map(|(name, need)| {
                        let count = need.customers.len();
                        NeedReport {
                            name,
                            customer_count: count,
                            percentage: if denominator == 0 {
                                0.0
                            } else {
                                100.0 * count as f64 / denominator as f64
                            },
                            qualifies: denominator > 0 && count > denominator / 4,
                            variants: need.variants.into_iter().collect(),
                            message_indices: need.messages.into_iter().collect(),
                        }
                    })
                    .collect();
                ProductReport {
                    product_id,
                    denominator,
                    needs,
                }
            })
            .collect();
        let mut report = Report {
            id: 0,
            start,
            end,
            input,
            classifications,
            products,
        };
        let tx = self.connection.transaction()?;
        tx.execute("INSERT INTO analysis_reports(report) VALUES ('')", [])?;
        report.id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE analysis_reports SET report = ?1 WHERE id = ?2",
            params![serde_json::to_string(&report)?, report.id],
        )?;
        tx.commit()?;
        Ok(report)
    }
    pub fn report(&self, id: i64) -> Result<Report> {
        let json: String = self
            .connection
            .query_row(
                "SELECT report FROM analysis_reports WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .context("report not found")?;
        let report: Report = serde_json::from_str(&json)?;
        if report.id != id {
            bail!("stored report ID does not match");
        }
        Ok(report)
    }
}
