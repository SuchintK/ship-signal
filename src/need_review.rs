use crate::{NeedReviewItem, Report, Workflow};
use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    Existing,
    New,
    Uncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NeedSuggestion {
    pub kind: SuggestionKind,
    pub identity: Option<String>,
    pub reason: String,
}

// Feedback and model output are untrusted; do not let them emit terminal controls.
fn display(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().to_string().chars().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}
fn read_line(
    input: &mut impl BufRead,
    output: &mut impl Write,
    prompt: &str,
) -> Result<Option<String>> {
    write!(output, "{prompt}")?;
    output.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    Ok(Some(line.trim().to_string()))
}

impl Workflow {
    /// Interactively confirm model suggestions. Each decision is saved before moving on.
    pub fn review_needs(
        &mut self,
        product_id: &str,
        id: i64,
        input: &mut impl BufRead,
        output: &mut impl Write,
        include_deferred: bool,
        mut suggest: impl FnMut(&NeedReviewItem) -> Result<NeedSuggestion>,
    ) -> Result<Option<Report>> {
        let queue = self.need_review_queue(product_id, id)?;
        let deferred: std::collections::BTreeSet<(String, String)> = {
            let mut statement = self.connection.prepare(
                "SELECT product_id, need FROM need_review_deferrals WHERE report_id = ?1 AND product_id = ?2",
            )?;
            let rows = statement.query_map(rusqlite::params![id, product_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let queue: Vec<_> = queue
            .into_iter()
            .filter(|item| {
                include_deferred
                    || !deferred.contains(&(item.product_id.clone(), item.need.clone()))
            })
            .collect();
        writeln!(output, "Report {id}: {} needs to review", queue.len())?;
        for original in queue {
            // Refresh candidates after each decision, including newly created identities.
            let Some(item) = self
                .need_review_queue(product_id, id)?
                .into_iter()
                .find(|item| item.product_id == original.product_id && item.need == original.need)
            else {
                continue;
            };
            let suggestion = suggest(&item)?;
            match suggestion.kind {
                SuggestionKind::Existing => ensure!(
                    suggestion
                        .identity
                        .as_ref()
                        .is_some_and(|id| item.candidates.iter().any(|c| &c.identity == id)),
                    "model suggested an unknown need identity for this product"
                ),
                SuggestionKind::New | SuggestionKind::Uncertain => ensure!(
                    suggestion.identity.is_none(),
                    "model returned an identity for a new or uncertain suggestion"
                ),
            }
            writeln!(
                output,
                "\nProduct: {}\nNeed: {}\nEvidence: {} messages from {} identified customers",
                display(&item.product_id),
                display(&item.need),
                item.messages.len(),
                item.customer_count
            )?;
            if !item.variants.is_empty() {
                writeln!(
                    output,
                    "Variants: {}",
                    item.variants
                        .iter()
                        .map(|v| display(v))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
            }
            for message in &item.messages {
                writeln!(output, "  - {}", display(message))?;
            }
            match suggestion.kind {
                SuggestionKind::Existing => writeln!(
                    output,
                    "Suggested match: {}",
                    display(suggestion.identity.as_deref().unwrap_or_default())
                )?,
                SuggestionKind::New => writeln!(output, "Suggestion: new need")?,
                SuggestionKind::Uncertain => writeln!(output, "Suggestion: uncertain")?,
            }
            writeln!(output, "Reason: {}", display(&suggestion.reason))?;
            for candidate in &item.candidates {
                writeln!(
                    output,
                    "  {}: {}",
                    display(&candidate.identity),
                    candidate
                        .needs
                        .iter()
                        .map(|s| display(s))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
            }
            loop {
                let Some(choice) = read_line(
                    input,
                    output,
                    "[1] Confirm suggested match [2] Choose existing [3] Create new [4] Leave unresolved [q] Quit: ",
                )?
                else {
                    return Ok(None);
                };
                match choice.as_str() {
                    "1" => match &suggestion.identity {
                        Some(identity) => {
                            self.assign_need_identity(&item.product_id, &item.need, identity)?;
                            break;
                        }
                        None => writeln!(
                            output,
                            "No existing match to confirm; choose another option."
                        )?,
                    },
                    "2" => {
                        let Some(identity) = read_line(input, output, "Existing need ID: ")? else {
                            return Ok(None);
                        };
                        if !item
                            .candidates
                            .iter()
                            .any(|candidate| candidate.identity == identity)
                        {
                            writeln!(output, "Unknown need ID for this product.")?;
                            continue;
                        }
                        self.assign_need_identity(&item.product_id, &item.need, &identity)?;
                        break;
                    }
                    "3" => {
                        let Some(identity) = read_line(input, output, "New need ID: ")? else {
                            return Ok(None);
                        };
                        if identity.is_empty()
                            || item
                                .candidates
                                .iter()
                                .any(|candidate| candidate.identity == identity)
                        {
                            writeln!(
                                output,
                                "Enter a nonempty new ID that is not already in use for this product."
                            )?;
                            continue;
                        }
                        self.assign_need_identity(&item.product_id, &item.need, &identity)?;
                        break;
                    }
                    "4" => {
                        self.connection.execute("INSERT INTO need_review_deferrals (report_id, product_id, need) VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING", rusqlite::params![id, item.product_id, item.need])?;
                        break;
                    }
                    "q" | "Q" => {
                        writeln!(
                            output,
                            "Review stopped. Confirmed decisions are saved; resume using the same report ID."
                        )?;
                        return Ok(None);
                    }
                    _ => writeln!(output, "Choose 1, 2, 3, 4, or q.")?,
                }
            }
        }
        let reviewed = self.review_report(product_id, id)?;
        writeln!(
            output,
            "Saved reviewed report {} (from {}).",
            reviewed.id, id
        )?;
        if reviewed.productReport.needs_identity_review {
            writeln!(
                output,
                "Some needs remain unresolved. Demand is provisional for this product."
            )?;
        }
        Ok(Some(reviewed))
    }
}
