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
pub struct NeedIdentityAlias {
    pub product_id: String,
    pub label: String,
    pub identity: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeedReport {
    pub name: String,
    #[serde(default)]
    pub identity: Option<String>,
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
    #[serde(default = "review_required_for_legacy_report")]
    pub needs_identity_review: bool,
    pub needs: Vec<NeedReport>,
}
fn review_required_for_legacy_report() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Report {
    pub id: i64,
    #[serde(default)]
    pub reviewed_from: Option<i64>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub input: AnalysisInput,
    pub classifications: Vec<Classification>,
    pub products: Vec<ProductReport>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum GroupKey {
    Mapped(String),
    Unresolved(String),
}

#[derive(Default)]
struct ProductDemand {
    customers: BTreeSet<String>,
    needs: BTreeMap<GroupKey, NeedDemand>,
}
#[derive(Default)]
struct NeedDemand {
    labels: BTreeSet<String>,
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
        connection.execute_batch("CREATE TABLE IF NOT EXISTS analysis_reports (id INTEGER PRIMARY KEY, report TEXT NOT NULL); CREATE TABLE IF NOT EXISTS need_identity_aliases (product_id TEXT NOT NULL, label TEXT NOT NULL, identity TEXT NOT NULL, PRIMARY KEY (product_id, label));")?;
        Ok(Self { connection })
    }
    /// Assign a product-scoped identity to a label after human review.
    /// Conflicting assignments fail; corrections require the explicit guarded operation.
    pub fn assign_need_identity(
        &self,
        product_id: &str,
        label: &str,
        identity: &str,
    ) -> Result<()> {
        ensure!(
            !product_id.trim().is_empty()
                && !label.trim().is_empty()
                && !identity.trim().is_empty(),
            "product, label, and identity must be nonempty"
        );
        self.connection.execute("INSERT INTO need_identity_aliases (product_id, label, identity) VALUES (?1, ?2, ?3) ON CONFLICT(product_id, label) DO NOTHING", params![product_id, label, identity])?;
        let stored: String = self.connection.query_row(
            "SELECT identity FROM need_identity_aliases WHERE product_id = ?1 AND label = ?2",
            params![product_id, label],
            |row| row.get(0),
        )?;
        ensure!(
            stored == identity,
            "label already has a different identity; review the existing mapping"
        );
        Ok(())
    }
    pub fn correct_need_identity(
        &self,
        product_id: &str,
        label: &str,
        previous: &str,
        identity: &str,
    ) -> Result<()> {
        ensure!(
            !product_id.trim().is_empty()
                && !label.trim().is_empty()
                && !previous.trim().is_empty()
                && !identity.trim().is_empty(),
            "product, label, previous identity, and identity must be nonempty"
        );
        let count = self.connection.execute("UPDATE need_identity_aliases SET identity = ?1 WHERE product_id = ?2 AND label = ?3 AND identity = ?4", params![identity, product_id, label, previous])?;
        ensure!(
            count == 1,
            "mapping not found or previous identity changed; list mappings and review before retrying"
        );
        Ok(())
    }
    pub fn need_identity_aliases(&self) -> Result<Vec<NeedIdentityAlias>> {
        let mut statement = self.connection.prepare("SELECT product_id, label, identity FROM need_identity_aliases ORDER BY product_id, label")?;
        let rows = statement.query_map([], |row| {
            Ok(NeedIdentityAlias {
                product_id: row.get(0)?,
                label: row.get(1)?,
                identity: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
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
        let classifications = classify(&input.feedback)?;
        self.complete_analysis(start, end, input, classifications, None)
    }
    /// Recalculate grouping from saved evidence with the current reviewed mappings.
    pub fn review_report(&mut self, id: i64) -> Result<Report> {
        let previous = self.report(id)?;
        self.complete_analysis(
            previous.start,
            previous.end,
            previous.input,
            previous.classifications,
            Some(id),
        )
    }
    fn complete_analysis(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        input: AnalysisInput,
        mut classifications: Vec<Classification>,
        reviewed_from: Option<i64>,
    ) -> Result<Report> {
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
        let aliases: BTreeMap<(String, String), String> = {
            let mut statement = self
                .connection
                .prepare("SELECT product_id, label, identity FROM need_identity_aliases")?;
            let rows =
                statement.query_map([], |row| Ok(((row.get(0)?, row.get(1)?), row.get(2)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let mut products: BTreeMap<String, ProductDemand> = BTreeMap::new();
        for (row, classification) in input.feedback.iter().zip(&classifications) {
            let product = products.entry(row.product_id.clone()).or_default();
            if !classification.needs.is_empty() {
                if let Some(customer) = &row.customer_id {
                    product.customers.insert(customer.clone());
                }
            }
            for need in &classification.needs {
                let identity = aliases.get(&(row.product_id.clone(), need.name.clone()));
                let key = match identity {
                    Some(identity) => GroupKey::Mapped(identity.clone()),
                    None => GroupKey::Unresolved(need.name.clone()),
                };
                let group = product.needs.entry(key).or_default();
                group.labels.insert(need.name.clone());
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
                let needs_identity_review = product
                    .needs
                    .keys()
                    .any(|key| matches!(key, GroupKey::Unresolved(_)));
                let needs = product
                    .needs
                    .into_iter()
                    .map(|(key, need)| {
                        let name = need.labels.iter().next().cloned().unwrap_or_default();
                        let count = need.customers.len();
                        NeedReport {
                            name,
                            identity: match key {
                                GroupKey::Mapped(identity) => Some(identity),
                                GroupKey::Unresolved(_) => None,
                            },
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
                    needs_identity_review,
                    needs,
                }
            })
            .collect();
        let mut report = Report {
            id: 0,
            reviewed_from,
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
