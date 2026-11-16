use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

mod need_review;
pub mod postgres_source;
pub use need_review::{NeedSuggestion, SuggestionKind};

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
    pub need: String,
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
    pub need: String,
    pub identity: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeedCandidate {
    pub identity: String,
    pub needs: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeedReviewItem {
    pub product_id: String,
    pub need: String,
    pub customer_count: usize,
    pub messages: Vec<String>,
    pub variants: Vec<String>,
    pub candidates: Vec<NeedCandidate>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeedReport {
    pub need: String,
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
    pub needs_identity_review: bool,
    pub needs: Vec<NeedReport>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Report {
    pub id: i64,
    pub reviewed_from: Option<i64>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub input: AnalysisInput,
    pub classifications: Vec<Classification>,
    pub products: Vec<ProductReport>,
}

// The row's primary key is the report ID; it is not duplicated in the JSON.
#[derive(Serialize, Deserialize)]
struct StoredReport {
    reviewed_from: Option<i64>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    input: AnalysisInput,
    classifications: Vec<Classification>,
    products: Vec<ProductReport>,
}

impl StoredReport {
    fn into_report(self, id: i64) -> Report {
        Report {
            id,
            reviewed_from: self.reviewed_from,
            start: self.start,
            end: self.end,
            input: self.input,
            classifications: self.classifications,
            products: self.products,
        }
    }
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
    need_texts: BTreeSet<String>,
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
        connection.execute_batch("CREATE TABLE IF NOT EXISTS analysis_reports (id INTEGER PRIMARY KEY, report TEXT NOT NULL); CREATE TABLE IF NOT EXISTS need_identity_aliases (product_id TEXT NOT NULL, need TEXT NOT NULL, identity TEXT NOT NULL, PRIMARY KEY (product_id, need)); CREATE TABLE IF NOT EXISTS need_review_deferrals (report_id INTEGER NOT NULL, product_id TEXT NOT NULL, need TEXT NOT NULL, PRIMARY KEY (report_id, product_id, need));")?;
        Ok(Self { connection })
    }
    /// Assign a product-scoped identity to a need after human review.
    /// Conflicting assignments fail; corrections require the explicit guarded operation.
    pub fn assign_need_identity(&self, product_id: &str, need: &str, identity: &str) -> Result<()> {
        ensure!(
            !product_id.trim().is_empty() && !need.trim().is_empty() && !identity.trim().is_empty(),
            "product, need, and identity must be nonempty"
        );
        self.connection.execute("INSERT INTO need_identity_aliases (product_id, need, identity) VALUES (?1, ?2, ?3) ON CONFLICT(product_id, need) DO NOTHING", params![product_id, need, identity])?;
        let stored: String = self.connection.query_row(
            "SELECT identity FROM need_identity_aliases WHERE product_id = ?1 AND need = ?2",
            params![product_id, need],
            |row| row.get(0),
        )?;
        ensure!(
            stored == identity,
            "need already has a different identity; review the existing mapping"
        );
        Ok(())
    }
    pub fn correct_need_identity(
        &self,
        product_id: &str,
        need: &str,
        previous: &str,
        identity: &str,
    ) -> Result<()> {
        ensure!(
            !product_id.trim().is_empty()
                && !need.trim().is_empty()
                && !previous.trim().is_empty()
                && !identity.trim().is_empty(),
            "product, need, previous identity, and identity must be nonempty"
        );
        let count = self.connection.execute("UPDATE need_identity_aliases SET identity = ?1 WHERE product_id = ?2 AND need = ?3 AND identity = ?4", params![identity, product_id, need, previous])?;
        ensure!(
            count == 1,
            "mapping not found or previous identity changed; list mappings and review before retrying"
        );
        Ok(())
    }
    pub fn need_identity_aliases(&self) -> Result<Vec<NeedIdentityAlias>> {
        let mut statement = self.connection.prepare("SELECT product_id, need, identity FROM need_identity_aliases ORDER BY product_id, need")?;
        let rows = statement.query_map([], |row| {
            Ok(NeedIdentityAlias {
                product_id: row.get(0)?,
                need: row.get(1)?,
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
    /// List unresolved needs from saved evidence and product-scoped confirmed identities.
    pub fn need_review_queue(&self, id: i64) -> Result<Vec<NeedReviewItem>> {
        let report = self.report(id)?;
        let aliases = self.need_identity_aliases()?;
        let mut result = Vec::new();
        for product in &report.products {
            let mut candidates: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for alias in aliases
                .iter()
                .filter(|a| a.product_id == product.product_id)
            {
                candidates
                    .entry(alias.identity.clone())
                    .or_default()
                    .push(alias.need.clone());
            }
            let candidates: Vec<NeedCandidate> = candidates
                .into_iter()
                .map(|(identity, needs)| NeedCandidate { identity, needs })
                .collect();
            for report_need in product.needs.iter().filter(|n| n.identity.is_none()) {
                if aliases
                    .iter()
                    .any(|a| a.product_id == product.product_id && a.need == report_need.need)
                {
                    continue;
                }
                let messages = report_need
                    .message_indices
                    .iter()
                    .map(|&i| {
                        report
                            .input
                            .feedback
                            .get(i)
                            .map(|row| row.text.clone())
                            .context("saved report references a missing message")
                    })
                    .collect::<Result<Vec<_>>>()?;
                result.push(NeedReviewItem {
                    product_id: product.product_id.clone(),
                    need: report_need.need.clone(),
                    customer_count: report_need.customer_count,
                    messages,
                    variants: report_need.variants.clone(),
                    candidates: candidates.clone(),
                });
            }
        }
        Ok(result)
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
        ensure!(
            classifications.len() == input.feedback.len(),
            "classifier must return one classification per message"
        );
        classifications.sort_by_key(|c| c.message_index);
        for (i, classification) in classifications.iter().enumerate() {
            ensure!(
                classification.message_index == i,
                "classifier returned missing, duplicate, or invalid message index"
            );
            for request in &classification.needs {
                ensure!(
                    !request.need.trim().is_empty(),
                    "classifier returned an empty need"
                );
                ensure!(
                    request
                        .variant
                        .as_ref()
                        .is_none_or(|v| !v.trim().is_empty()),
                    "classifier returned an empty variant"
                );
            }
        }
        let aliases: BTreeMap<(String, String), String> = {
            let mut statement = self
                .connection
                .prepare("SELECT product_id, need, identity FROM need_identity_aliases")?;
            let rows =
                statement.query_map([], |row| Ok(((row.get(0)?, row.get(1)?), row.get(2)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let mut products: BTreeMap<String, ProductDemand> = BTreeMap::new();
        // zip doesn't match using message_index. That is why the earlier code first sorts
        // classifications by index and validates their count and indices.
        for (row, classification) in input.feedback.iter().zip(&classifications) {
            let product = products.entry(row.product_id.clone()).or_default();
            if !classification.needs.is_empty() {
                if let Some(customer) = &row.customer_id {
                    product.customers.insert(customer.clone());
                }
            }
            for request in &classification.needs {
                let identity = aliases.get(&(row.product_id.clone(), request.need.clone()));
                let key = match identity {
                    Some(identity) => GroupKey::Mapped(identity.clone()),
                    None => GroupKey::Unresolved(request.need.clone()),
                };
                let group = product.needs.entry(key).or_default();
                group.need_texts.insert(request.need.clone());
                if let Some(customer) = &row.customer_id {
                    group.customers.insert(customer.clone());
                }
                if let Some(variant) = &request.variant {
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
                    .map(|(key, demand)| {
                        let need_text =
                            demand.need_texts.iter().next().cloned().unwrap_or_default();
                        let count = demand.customers.len();
                        NeedReport {
                            need: need_text,
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
                            variants: demand.variants.into_iter().collect(),
                            message_indices: demand.messages.into_iter().collect(),
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
        let report = StoredReport {
            reviewed_from,
            start,
            end,
            input,
            classifications,
            products,
        };
        let json = serde_json::to_string(&report)?;
        let id: i64 = self.connection.query_row(
            "INSERT INTO analysis_reports(report) VALUES (?1) RETURNING id",
            params![json],
            |row| row.get(0),
        )?;
        Ok(report.into_report(id))
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
        let report: StoredReport = serde_json::from_str(&json)?;
        Ok(report.into_report(id))
    }
}
