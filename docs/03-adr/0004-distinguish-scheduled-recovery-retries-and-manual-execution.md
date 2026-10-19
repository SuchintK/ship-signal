# ADR-0004: Distinguish scheduled execution, recovery, retries, and manual execution

- **Status:** Accepted
- **Date:** 2026-10-19

## Context

Ship Signal automatically analyzes feedback Friday and implements approved work during a configured weekend window. Customer-hosted downtime, delayed notifications, failed implementations, and unfinished queues need predictable handling without requiring a person to intervene after every service interruption.

Approval authorizes scope; it does not by itself request immediate execution. The design must distinguish continuing interrupted work from granting another budget, and distinguish a grace period for ongoing work from permission to start additional tickets outside the schedule. These distinctions affect durable workflow state, authorization, budget accounting, and command shutdown.

## Decision

Use four distinct execution policies:

1. **Automatic scheduled execution:** catch up missed Friday analysis automatically and start the Saturday queue late while its execution window remains open. Preserve the original feedback interval and scheduling boundaries. Determine Saturday eligibility from verified actual approval time, not notification arrival. Timezone changes apply next weekly cycle; the current cycle retains its boundaries and assigned timezone's daylight-saving rules.
2. **Automatic recovery:** safely resume interrupted implementation within the same permitted weekend window using saved progress and the existing budget. Restarting the service does not reset consumed attempt, time, or cost limits. Establish the actual execution state before continuing, including subprocesses that may have survived the interruption.
3. **Human-authorized implementation retry:** failed, exhausted, or unfinished implementation requires an explicit human decision to try again. A retry can run during the current weekend and receives a fresh bounded budget, while retaining the applicable execution deadline. This authorization is distinct from recovering an interrupted run or retrying an individual external API operation.
4. **Explicit manual execution:** a configured approver can request execution of any approved ticket outside the scheduled window, including a ticket approved after Saturday's deadline or an untouched queued ticket. Every manual run requires an explicit end deadline and remains subject to bounded budgets and repository concurrency limits. Support both configured Linear actions and CLI commands with equivalent configured-approver checks.

At Sunday's cutoff, stop starting implementations through the automatic queue. Allow implementations already underway to continue within a bounded, configurable grace period, then stop remaining scheduled execution and preserve progress. The grace period does not admit untouched tickets or reset budgets. Starting additional tickets after the cutoff requires explicit manual execution, governed by its own deadline.

Retain one active implementation per repository; different repositories may run independently. An earlier unmerged PR does not block a new implementation on a separate branch. Untouched tickets can remain for the next weekend or be explicitly started manually. The detailed schedule and queue ordering remain specified in the product design.

Persist the boundaries, eligibility, progress, and budget consumption needed to preserve these distinctions across restarts. Manual execution overrides scheduling, not approval or scope. The basic approval policy in ADR-0002 and storage/deployment constraints in ADR-0003 remain in effect.

## Alternatives considered

- **Skip missed schedules or require a person to resume every interruption:** simplifies recovery policy and gives people direct control, but short outages can lose an entire weekend or require weekend attention. Automatic catch-up and recovery were selected instead.
- **Require all implementation retries to wait until next weekend:** keeps scheduling stricter, but prevents a person from unblocking failed work during an otherwise available execution window.
- **Enforce a firm Sunday stop without grace or manual execution:** offers the strictest end time, but interrupts ongoing work and prevents deliberate execution outside the regular schedule. Bounded grace and separately authorized manual execution were selected instead of unrestricted automatic extension.
- **Block new implementation until an earlier PR is merged or closed:** simplifies coordination of pending changes, but makes human review latency block repository throughput. Separate branches and one active implementation at a time were retained instead.

## Consequences

- Routine outages can be recovered without human intervention while preserving the original eligibility boundaries and consumed budgets.
- People can explicitly authorize another bounded attempt or choose a manual execution window without changing the recurring schedule or reapproving untouched work solely because it was not started.
- Execution can extend beyond Sunday's nominal cutoff through grace or a manual request. Operators must be able to distinguish those permissions and their deadlines.
- Each human-authorized retry grants a fresh budget; repeated authorizations can increase total spending. Automatic restart must not create that permission implicitly.
- Linear and CLI requests need consistent actor verification and handling of duplicate requests. Deadline enforcement requires controlling active execution, not merely pausing the workflow.
- Multiple unmerged PRs may overlap or conflict even though implementations run sequentially. This decision does not establish a branch/base-commit or conflict-resolution mechanism.

## Remaining specifications

Numerical budget, cutoff, and grace defaults; time/cost accounting; Linear/CLI syntax and actor verification; durable record schemas; command shutdown and safe resumption mechanisms; and treatment of multiple missed analysis cycles after prolonged downtime remain open. Catch-up must preserve weekly feedback intervals rather than count historical submissions as current-week feedback. This ADR records the execution policy, not a completed recovery implementation or new scope-versioning controls.

## References

- [Product design: weekly analysis](../01-grill-session/README.md#4-weekly-analysis-and-demand-threshold)
- [Product design: weekend execution and failure handling](../01-grill-session/README.md#10-weekend-execution-and-failure-handling)
- [Technical decision point 4: weekend scheduling and recovery](../02-technical-decisions/README.md#4-weekend-scheduling-and-recovery)
- [ADR-0002: defer versioned approval for MVP](0002-defer-versioned-approval-for-mvp.md)
- [ADR-0003: use SQLite for single-host MVP application state](0003-use-sqlite-for-single-host-mvp-state.md)
