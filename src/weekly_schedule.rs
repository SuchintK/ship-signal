use crate::{Classification, Feedback, FeedbackSource, Workflow, validate_product_id};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Datelike, Days, NaiveDate, Utc, Weekday};
use jiff::{
    Timestamp, civil,
    tz::{TimeZone, TimeZoneDatabase},
};
use rusqlite::{
    Connection, OptionalExtension, Row, params,
    types::{FromSql, FromSqlError, FromSqlResult, ValueRef},
};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

pub(super) fn initialize(connection: &Connection) -> Result<()> {
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS weekly_cycles (
            product_id TEXT NOT NULL,
            friday TEXT NOT NULL,
            timezone TEXT NOT NULL,
            feedback_start TEXT NOT NULL,
            analysis_cutoff TEXT NOT NULL,
            approval_cutoff TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('planned', 'pending', 'completed', 'skipped')),
            report_id INTEGER UNIQUE REFERENCES analysis_reports(id),
            CHECK ((status = 'completed') = (report_id IS NOT NULL)),
            PRIMARY KEY (product_id, friday)
        );",
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WeeklyCycleStatus {
    Planned,
    Pending,
    Completed,
    Skipped,
}

impl FromSql for WeeklyCycleStatus {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "planned" => Ok(Self::Planned),
            "pending" => Ok(Self::Pending),
            "completed" => Ok(Self::Completed),
            "skipped" => Ok(Self::Skipped),
            status => Err(FromSqlError::Other(
                format!("invalid weekly cycle status {status}").into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WeeklyCycle {
    pub product_id: String,
    pub friday: NaiveDate,
    pub timezone: String,
    pub feedback_start: DateTime<Utc>,
    pub analysis_cutoff: DateTime<Utc>,
    pub approval_cutoff: DateTime<Utc>,
    pub status: WeeklyCycleStatus,
    pub report_id: Option<i64>,
}

impl WeeklyCycle {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            product_id: row.get(0)?,
            friday: row.get(1)?,
            timezone: row.get(2)?,
            feedback_start: row.get(3)?,
            analysis_cutoff: row.get(4)?,
            approval_cutoff: row.get(5)?,
            status: row.get(6)?,
            report_id: row.get(7)?,
        })
    }

    fn plan(
        product_id: &str,
        friday: NaiveDate,
        zone: &TimeZone,
        feedback_start: DateTime<Utc>,
    ) -> Result<Self> {
        let saturday = friday.succ_opt().context("Saturday date is out of range")?;
        let timezone = zone.iana_name().context("expected an IANA timezone")?;
        Ok(Self {
            product_id: product_id.to_owned(),
            friday,
            timezone: timezone.to_owned(),
            feedback_start,
            analysis_cutoff: cutoff(friday, zone)?,
            approval_cutoff: cutoff(saturday, zone)?,
            status: WeeklyCycleStatus::Planned,
            report_id: None,
        })
    }

    fn insert_plan(&self, connection: &Connection) -> Result<()> {
        connection.execute(
            "INSERT INTO weekly_cycles (product_id, friday, timezone, feedback_start,
                analysis_cutoff, approval_cutoff, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'planned')",
            params![
                self.product_id,
                self.friday,
                self.timezone,
                self.feedback_start,
                self.analysis_cutoff,
                self.approval_cutoff
            ],
        )?;
        Ok(())
    }
}

fn cutoff(date: NaiveDate, zone: &TimeZone) -> Result<DateTime<Utc>> {
    let timestamp = zone
        .to_zoned_datetime(civil::DateTime::new(
            date.year().try_into()?,
            date.month().try_into()?,
            date.day().try_into()?,
            9,
            0,
            0,
            0,
        )?)?
        .timestamp();
    DateTime::from_timestamp(timestamp.as_second(), 0).context("weekly cutoff is out of range")
}

fn previous_cutoff(friday: NaiveDate, zone: &TimeZone) -> Result<DateTime<Utc>> {
    let previous_friday = friday
        .checked_sub_days(Days::new(7))
        .context("previous Friday date is out of range")?;
    cutoff(previous_friday, zone)
}

const PRODUCT_CYCLES: &str =
    "SELECT product_id, friday, timezone, feedback_start, analysis_cutoff, approval_cutoff,
            status, report_id FROM weekly_cycles WHERE product_id = ?1";

impl Workflow {
    /// List the selected product's weekly cycles, newest first.
    pub fn weekly_cycles(&self, product_id: &str) -> Result<Vec<WeeklyCycle>> {
        validate_product_id(product_id)?;
        let mut statement = self
            .connection
            .prepare(&format!("{PRODUCT_CYCLES} ORDER BY friday DESC"))?;
        let cycles = statement
            .query_map([product_id], WeeklyCycle::from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(cycles)
    }
}

/// Owns scheduled analysis for a local state file while the scheduler is running.
pub struct WeeklyScheduler {
    _lock: File,
    workflow: Workflow,
    product_id: String,
    zone: TimeZone,
    time_zones: TimeZoneDatabase,
}

impl WeeklyScheduler {
    pub fn open(state: impl AsRef<Path>, product_id: &str, timezone: &str) -> Result<Self> {
        validate_product_id(product_id)?;
        let time_zones = TimeZoneDatabase::bundled();
        let zone = time_zones
            .get(timezone)
            .context("invalid schedule timezone")?;
        ensure!(zone.iana_name().is_some(), "expected an IANA timezone");
        let workflow = Workflow::open(&state)?;
        let mut lock_path = state.as_ref().canonicalize()?.into_os_string();
        lock_path.push(".weekly.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .context("open weekly scheduler lock")?;
        lock.try_lock().context("acquire weekly scheduler lock")?;
        Ok(Self {
            _lock: lock,
            workflow,
            product_id: product_id.to_owned(),
            zone,
            time_zones,
        })
    }

    /// Advance the schedule and save the original report if a cycle is due.
    pub fn tick(
        &mut self,
        now: DateTime<Utc>,
        source: &mut (impl FeedbackSource + ?Sized),
        classify: impl FnOnce(&[Feedback]) -> Result<Vec<Classification>>,
    ) -> Result<Option<WeeklyCycle>> {
        let Some(mut cycle) = self.due_cycle(now)? else {
            return Ok(None);
        };
        let report = self.workflow.analyzed_report(
            &cycle.product_id,
            source,
            cycle.feedback_start,
            cycle.analysis_cutoff,
            classify,
        )?;
        let tx = self.workflow.connection.transaction()?;
        let report = report.save(&tx, &cycle.product_id)?;
        let completed = tx.execute(
            "UPDATE weekly_cycles SET status = 'completed', report_id = ?3
             WHERE product_id = ?1 AND friday = ?2 AND status = 'pending'",
            params![cycle.product_id, cycle.friday, report.id],
        )?;
        ensure!(completed == 1, "weekly cycle is no longer pending");
        tx.commit()?;
        cycle.status = WeeklyCycleStatus::Completed;
        cycle.report_id = Some(report.id);
        Ok(Some(cycle))
    }

    fn due_cycle(&mut self, now: DateTime<Utc>) -> Result<Option<WeeklyCycle>> {
        let tx = self.workflow.connection.transaction()?;
        let planned = cycle_with_status(&tx, &self.product_id, "planned")?;
        let mut planned = match planned {
            Some(planned) => planned,
            None => {
                let first = first_cycle(now, &self.product_id, &self.zone)?;
                first.insert_plan(&tx)?;
                first
            }
        };
        while planned.analysis_cutoff <= now {
            tx.execute(
                "UPDATE weekly_cycles SET status = 'skipped'
                 WHERE product_id = ?1 AND status = 'pending'",
                [&self.product_id],
            )?;
            tx.execute(
                "UPDATE weekly_cycles SET status = 'pending'
                 WHERE product_id = ?1 AND friday = ?2 AND status = 'planned'",
                params![self.product_id, planned.friday],
            )?;
            let next_friday = planned
                .friday
                .checked_add_days(Days::new(7))
                .context("next Friday date is out of range")?;
            let zone = self.time_zones.get(&planned.timezone)?;
            planned = WeeklyCycle::plan(
                &self.product_id,
                next_friday,
                &zone,
                planned.analysis_cutoff,
            )?;
            planned.insert_plan(&tx)?;
        }
        // Apply a configuration change only once both possible cutoffs are still ahead.
        if Some(planned.timezone.as_str()) != self.zone.iana_name() {
            let revised = WeeklyCycle::plan(
                &self.product_id,
                planned.friday,
                &self.zone,
                planned.feedback_start,
            )?;
            if revised.analysis_cutoff > now {
                let has_previous: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM weekly_cycles WHERE product_id = ?1 AND friday < ?2)",
                    params![self.product_id, revised.friday],
                    |row| row.get(0),
                )?;
                let feedback_start = if has_previous {
                    revised.feedback_start
                } else {
                    previous_cutoff(revised.friday, &self.zone)?
                };
                tx.execute(
                    "UPDATE weekly_cycles SET timezone = ?3, analysis_cutoff = ?4,
                         approval_cutoff = ?5, feedback_start = ?6
                     WHERE product_id = ?1 AND friday = ?2 AND status = 'planned'",
                    params![
                        self.product_id,
                        revised.friday,
                        revised.timezone,
                        revised.analysis_cutoff,
                        revised.approval_cutoff,
                        feedback_start
                    ],
                )?;
            }
        }
        let pending = cycle_with_status(&tx, &self.product_id, "pending")?;
        tx.commit()?;
        Ok(pending.filter(|cycle| cycle.analysis_cutoff <= now))
    }
}

fn cycle_with_status(
    connection: &Connection,
    product_id: &str,
    status: &str,
) -> Result<Option<WeeklyCycle>> {
    Ok(connection
        .query_row(
            &format!("{PRODUCT_CYCLES} AND status = ?2 ORDER BY friday DESC LIMIT 1"),
            params![product_id, status],
            WeeklyCycle::from_row,
        )
        .optional()?)
}

fn first_cycle(now: DateTime<Utc>, product_id: &str, zone: &TimeZone) -> Result<WeeklyCycle> {
    let local = zone.to_datetime(Timestamp::from_second(now.timestamp())?);
    let today = NaiveDate::from_ymd_opt(
        local.year().into(),
        local.month().try_into()?,
        local.day().try_into()?,
    )
    .context("local schedule date is out of range")?;
    // Resolve the latest local Friday before advancing. On a date-line change
    // its compatible cutoff can fall on Saturday.
    let days_since_friday =
        (today.weekday().num_days_from_monday() + 7 - Weekday::Fri.num_days_from_monday()) % 7;
    let mut friday = today
        .checked_sub_days(Days::new(days_since_friday.into()))
        .context("Friday date is out of range")?;
    if cutoff(friday, zone)? < now {
        friday = friday
            .checked_add_days(Days::new(7))
            .context("next Friday date is out of range")?;
    }
    WeeklyCycle::plan(product_id, friday, zone, previous_cutoff(friday, zone)?)
}
