use super::{Assessment, AssessmentKind, InvestigationSources, SearchContext};
use crate::Report;
use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RejectionMatch {
    pub ticket_id: String,
    pub coverage: RejectionCoverage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RejectionCoverage {
    Full,
    Partial,
    Uncertain,
}

/// The status transition recorded by Linear for this rejection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectionDecision {
    pub ticket_id: String,
    pub history_id: String,
    pub decided_at: DateTime<Utc>,
    pub actor_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectionReason {
    pub comment_id: String,
    pub text: String,
}

/// The decisions and reason used to prepare a potential Linear write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectionEvidence {
    /// Ordered newest first. The first decision is the one linked on a new proposal.
    pub decisions: Vec<RejectionDecision>,
    pub reason: Option<RejectionReason>,
}

#[derive(Clone, Copy)]
pub(super) struct WeeklyEvidence {
    end: DateTime<Utc>,
    next_cutoff: Option<DateTime<Utc>>,
}

impl WeeklyEvidence {
    pub fn load(connection: &Connection, report: &Report, root_id: i64) -> Result<Self> {
        let scheduled_cutoff: Option<Option<DateTime<Utc>>> = connection
            .query_row(
                "SELECT (SELECT newer.analysis_cutoff FROM weekly_cycles AS newer
                         WHERE newer.product_id = cycle.product_id AND newer.friday > cycle.friday
                         ORDER BY newer.friday LIMIT 1)
                 FROM weekly_cycles AS cycle WHERE cycle.product_id = ?1 AND cycle.report_id = ?2",
                params![report.product_id, root_id],
                |row| row.get(0),
            )
            .optional()?;
        let next_cutoff = match scheduled_cutoff {
            Some(cutoff) => cutoff,
            None if report.end - report.start == TimeDelta::weeks(1) => {
                report.end.checked_add_signed(TimeDelta::weeks(1))
            }
            None => None,
        };
        Ok(Self {
            end: report.end,
            next_cutoff,
        })
    }

    pub fn qualifies_at(self, now: DateTime<Utc>, expiry: DateTime<Utc>) -> bool {
        expiry <= self.end && self.end <= now && self.next_cutoff.is_some_and(|cutoff| now < cutoff)
    }
}

impl RejectionDecision {
    pub(super) fn expiry(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        if self.decided_at > now {
            return None;
        }
        self.decided_at.checked_add_signed(TimeDelta::days(90))
    }
}

pub(super) enum Outcome {
    Proceed(Option<RejectionEvidence>),
    Suppressed(String),
    Review(String),
}

pub(super) fn evaluate(
    assessment: &Assessment,
    context: &SearchContext<'_>,
    sources: &mut impl InvestigationSources,
    weekly: WeeklyEvidence,
) -> Result<Outcome> {
    let mut full = Vec::new();
    let mut partial = false;
    let mut uncertain = false;
    for rejection in &assessment.rejection_matches {
        let Some(ticket) = context.tickets.iter().find(|ticket| {
            ticket.id == rejection.ticket_id
                && context.not_needed_state_ids.contains(&ticket.state_id)
        }) else {
            return Ok(Outcome::Review(
                "Investigation cited a rejection outside the product's Not needed issues.".into(),
            ));
        };
        match rejection.coverage {
            RejectionCoverage::Full => full.push(ticket),
            RejectionCoverage::Partial => partial = true,
            RejectionCoverage::Uncertain => uncertain = true,
        }
    }

    // A rejected ticket must be assessed by coverage before it can affect a write.
    if assessment.matching_ticket_ids.iter().any(|id| {
        context.tickets.iter().any(|ticket| {
            &ticket.id == id && context.not_needed_state_ids.contains(&ticket.state_id)
        })
    }) {
        return Ok(Outcome::Review(
            "Investigation used a rejected ticket as ordinary matching work.".into(),
        ));
    }
    if partial {
        return Ok(Outcome::Review(
            "The proposal combines rejected behavior with other behavior. Review its scope.".into(),
        ));
    }
    if uncertain {
        return Ok(Outcome::Review(
            "The overlap with rejected work needs review.".into(),
        ));
    }
    if full.is_empty() {
        return Ok(Outcome::Proceed(None));
    }

    full.sort_by(|left, right| left.id.cmp(&right.id));
    full.dedup_by(|left, right| left.id == right.id);
    let mut decisions = Vec::new();
    let mut missing = false;
    let mut active_until = None;
    let mut fresh = true;
    let now = sources.now();
    for ticket in full {
        let Some(decision) = sources.rejection_decision(ticket, context.not_needed_state_ids)?
        else {
            missing = true;
            continue;
        };
        let Some(expiry) = decision
            .expiry(now)
            .filter(|_| decision.ticket_id == ticket.id)
        else {
            missing = true;
            continue;
        };
        if now < expiry {
            active_until = active_until.max(Some(expiry));
        }
        fresh &= weekly.qualifies_at(now, expiry);
        decisions.push(decision);
    }
    if let Some(expiry) = active_until {
        return Ok(Outcome::Suppressed(format!(
            "The rejected-feature cooldown expires at {}.",
            expiry.to_rfc3339()
        )));
    }
    if missing {
        return Ok(Outcome::Review(
            "Linear history did not establish when this work was rejected.".into(),
        ));
    }
    if !fresh {
        return Ok(Outcome::Review("Reconsideration requires a current weekly report that closed at or after cooldown expiry.".into()));
    }

    decisions.sort_by(|left, right| {
        right
            .decided_at
            .cmp(&left.decided_at)
            .then_with(|| left.ticket_id.cmp(&right.ticket_id))
    });
    let reason = if matches!(assessment.kind, AssessmentKind::DistinctWork) {
        let Some(reason) = sources.rejection_reason(&decisions[0])? else {
            return Ok(Outcome::Review("The latest rejection needs one clear Rejection reason comment from the decision author.".into()));
        };
        Some(reason)
    } else {
        None
    };
    Ok(Outcome::Proceed(Some(RejectionEvidence {
        decisions,
        reason,
    })))
}
