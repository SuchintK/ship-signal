use crate::{AnalysisInput, Feedback};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const FEEDBACK_COLUMNS: &str = "product_id, feedback_source_id, source_message_id, customer_id, submitted_at, feedback_text, source_reference";

pub(crate) struct FeedbackRow {
    pub product_id: Option<String>,
    pub feedback_source_id: Option<String>,
    pub source_message_id: Option<String>,
    pub customer_id: Option<String>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub text: Option<String>,
    pub source_reference: Option<String>,
}

pub(crate) fn validate_rows(
    rows: impl IntoIterator<Item = Result<FeedbackRow>>,
) -> Result<AnalysisInput> {
    let mut diagnostics = BTreeMap::new();
    let mut candidates = Vec::new();
    for row in rows {
        let FeedbackRow {
            product_id,
            feedback_source_id: source_id,
            source_message_id: message_id,
            customer_id,
            submitted_at,
            text,
            source_reference,
        } = row?;
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
    // Use the same ordering for classifier indices regardless of database collation.
    candidates.sort_unstable_by(|a, b| {
        (&a.submitted_at, &a.feedback_source_id, &a.source_message_id).cmp(&(
            &b.submitted_at,
            &b.feedback_source_id,
            &b.source_message_id,
        ))
    });
    Ok(AnalysisInput {
        feedback: candidates,
        diagnostics,
    })
}
