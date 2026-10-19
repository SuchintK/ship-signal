# 01 — Grill session: settled product design

This record consolidates the decisions agreed during the grill session and the subsequent MVP review. It includes the later changes to database support, ticket eligibility, rejected-feature cooldowns, model independence, approval semantics, and MVP scope reductions. Earlier alternatives are not current requirements.

## 1. Product and audience

Ship Signal is a reusable product for teams running **B2C applications across industries**, initially aimed at small teams where a technical founder or engineering lead approves engineering work.

The complete workflow is:

**Customer feedback → weekly analysis → evidence-backed Linear ticket → human approval → weekend implementation → GitHub draft PR.**

Feedback analysis, ticket generation, and implementation belong to the same product. Ticket usefulness and implementation reliability should be evaluated separately; this does not mean separate products.

## 2. Initial integrations and configuration

- **PostgreSQL or MySQL:** the customer selects the feedback database during setup.
- **Linear:** tickets and approvals.
- **GitHub:** repositories and draft pull requests.

Map each connected repository to its relevant Linear teams/projects and existing labels. Include the team's existing tagging conventions in ticket investigation rather than requiring a replacement label system.

The workflow is industry-independent. Initial compatibility is limited to supported integrations and documented repository requirements rather than a promise to support every system immediately.

## 3. Uniform feedback schema

Both database integrations expose the same logical feedback schema through standardized, **read-only views** and database-specific connectors. Customers map their existing tables to these views; their underlying application schemas do not have to be uniform.

The accepted minimum logical field contract is:

| Field | Logical type | Required? |
| --- | --- | --- |
| Product ID | Nonempty text | Yes |
| Feedback source ID | Nonempty text | Yes |
| Source message ID | Nonempty text | Yes |
| Application customer ID | Nonempty text | No; null means unidentified |
| Submission time | UTC-normalized instant | Yes |
| Feedback text | Nonempty text | Yes |
| Source reference | Text, such as a URL or locator | No |

Treat identifiers as opaque text so numeric IDs, UUIDs, and existing string IDs fit the same contract. A null customer ID is valid unidentified feedback: preserve it but exclude it from demand percentages. Source references are optional because some sources have no link or locator. Exact SQL view definitions, database-specific types, and namespace configuration remain to be specified.

Exclude invalid feedback rows and continue analysis with the valid rows. Report the excluded-row count and validation reasons alongside the result, making clear that demand is calculated from the valid subset. Invalid rows contribute to neither feature customer counts nor the denominator. Valid rows remain subject to the normal demand threshold and ticket eligibility rules; invalid-row exclusions do not block ticket creation. A null customer ID is valid unidentified feedback, not an invalid row. If an excluded historical row is later corrected, the explicit historical recomputation policy applies.

Scope stable message identity by **product + feedback source + source message ID**. Scope customer identity by **product + application customer ID**, using the same application customer ID across channels. Identical message IDs from different sources remain separate evidence; messages from the same customer across those sources contribute to one distinct-customer count for the relevant feature/week. Identifiers must remain stable across reads rather than change with display names or row order.

Each submission time must identify an unambiguous instant normalized to UTC. During setup, the customer explicitly defines how existing source timestamps map to UTC; Ship Signal must not infer an unspecified source timezone. Weekly cutoffs continue to follow the cycle's assigned customer-configured timezone. Exact database field types and conversion/validation mechanics remain to be specified.

Document the contract in the README/getting-started guide and validate it during setup. The feedback connection must not modify the customer's original records.

## 4. Weekly analysis and demand threshold

The weekly evidence and historical correction policy is recorded in [ADR-0005](../03-adr/0005-preserve-weekly-feedback-evidence-and-require-explicit-historical-corrections.md).

Run analysis **Friday at 9 a.m. in a customer-configured timezone**, covering the interval from the previous Friday's cutoff to the current Friday's cutoff.

Use a **start-inclusive, end-exclusive** submission interval: `[previous Friday 09:00, current Friday 09:00)`. Feedback submitted exactly at the current Friday's 09:00 cutoff belongs to the new week. Adjacent intervals share a boundary without counting the same submission in both weeks.

If the service misses the scheduled analysis, run it automatically when service returns. Running late does not move the scheduled feedback interval or Saturday's approval deadline. See [weekend execution and failure handling](#10-weekend-execution-and-failure-handling) for scheduling and manual execution rules.

Calculate:

**Demand percentage = distinct customers requesting a particular feature ÷ distinct customers requesting any feature that week × 100.**

A feature qualifies for consideration only when demand is **strictly greater than 25%**. Exactly 25% does not qualify. There is no additional minimum customer count; show actual counts alongside percentages so the evidence is visible.

The denominator is feature-requesting customers, not all registered customers and not all feedback senders. Bug-only customers are excluded. A customer who submits both bugs and feature requests still counts once in the denominator.

Only that week's feedback contributes to the calculation. The rejected-feature cooldown described below does not change this counting window.

Save the feedback inputs and classification results used by each weekly analysis. Explain a completed result using those fixed copies rather than rereading live source records or rerunning the classifier. Later source edits or classification changes must not silently rewrite its evidence. Reusing saved classifications allows demand counts to be recalculated without assuming that another model invocation would produce identical classifications.

Additional per-result provenance tracking is not required for MVP. The proposed package of provider/model identifiers, prompt text or versions, analysis settings, software/contract versions, and input-capture metadata was declined as unnecessary overhead. The agreed saved weekly inputs, classifications, and invalid-row exclusion diagnostics remain required; exact model-run reproduction is not an MVP guarantee.

Historical corrections require an explicit request to recompute the affected week. Produce a corrected result distinguishable from the original and preserve the original subject to the retention policy. Late arrivals and identity, source-content, or classification corrections belong to the affected historical interval; they must not enter the current week's demand merely because the correction happened now. Source changes do not trigger automatic recomputation of completed weeks.

For a requested historical rerun, reread the feedback database for the original weekly interval using its original cutoffs and assigned timezone. Analyze the current message text and customer identities, include late-arriving submissions within that interval, and omit messages no longer exposed by the source, including deleted messages. Save the corrected inputs and classifications separately from the original result. Requiring the person to supply individual corrections to the saved inputs was not selected. The corrected result reflects the source as read for that rerun; it does not recreate the source as it existed at the original analysis time.

Request a historical rerun through the CLI and show the corrected report there when it completes. The report identifies the week and shows previous and corrected feature customer counts, denominator counts, percentages, invalid-row exclusion counts/reasons, and existing Linear ticket references where available. The rerun leaves existing Linear tickets unchanged; a person decides whether to update their evidence or demand numbers. Falling demand still does not invalidate an existing ticket. Exact command syntax and report formatting remain implementation details.

These copies remain subject to the retention policy; this does not require indefinite preservation. Omitting a deleted source message from a corrected result does not define deletion of previously saved copies; retention and deletion responsibilities remain part of technical decision point 9. The accepted feedback policy and remaining implementation details are tracked under [technical decision point 5](../02-technical-decisions/README.md#5-feedback-contract-reproducibility-and-corrections).

## 5. Customer identity, deduplication, and grouping

- Use the application's customer ID to identify a distinct B2C customer and link attributable messages across channels.
- Deduplicate **per customer, per feature, per week**.
- A customer requesting features A and B counts once toward each feature and once in the denominator.
- Repeated requests for the same feature count once, while every supporting message is preserved.
- Preserve unidentified feedback, but exclude it from percentages until identity is established.
- Count explicit feature requests. Inferred needs remain supporting context until a person confirms they belong to a feature request.

Percentages across features can add up to more than 100% because one customer can request multiple features.

Group related requests by the underlying need while retaining differences in requested behavior. For example, email and SMS requests can support the same back-in-stock need, but their delivery preferences must remain visible in the proposed ticket.

## 6. Investigating existing tickets and code

Search within the configured product's Linear teams/projects, considering its existing labels. Compare the underlying request against ticket titles, descriptions, acceptance criteria, statuses, linked PRs, and relevant repository code. Similar titles alone are not proof of duplicate work, and a completed ticket alone is not proof that the requested behavior exists.

| Finding | Outcome |
| --- | --- |
| The same work is already planned or underway | Attach supporting evidence and update demand counts instead of creating duplicate work. |
| The functionality already exists | Flag a possible bug or discoverability problem for review. |
| The request adds genuinely different behavior to related work | Propose a separate ticket linked to the existing work. |
| The match is uncertain | Flag it for human review instead of silently combining requests. |

The rejected-feature cooldown below takes precedence when matching a ticket marked "Not needed."

Adding evidence must not silently expand the scope of an approved ticket. The exact rules for using labels as strict filters versus supporting context remain to be finalized.

## 7. Bounded implementation scope

Initially propose **bounded changes using existing infrastructure**. Each ticket must define proposed behavior, assumptions, acceptance criteria, and scope boundaries. Material ambiguities must be resolved before implementation.

Examples discussed:

- **Email an existing PDF export:** can qualify when PDF generation and email delivery already exist. Introducing an email provider, credentials, and delivery architecture requires additional engineering design.
- **Back-in-stock notifications:** can qualify when the relevant inventory events and notification infrastructure already exist. Designing new event processing or messaging infrastructure requires additional engineering design.

New application code, tables, and migrations can still qualify. The constraint is a clear, limited change within supported capabilities, not a prohibition on creating new code.

If implementation requires expanded scope or new infrastructure, stop and report it rather than silently broadening the approved work.

## 8. Ticket eligibility and rejected-feature cooldown

**Once a ticket is created, falling demand does not invalidate it.** Later weekly scans must not mark a created ticket ineligible merely because its percentage falls to 25% or below.

When a ticket is marked **"Not needed"**:

1. Suppress proposals for the same or similar feature for **90 days from that decision**.
2. After the cooldown, the feature must exceed **25% again using that week's feedback** before reconsideration.
3. Create a **new ticket**, linking to the older ticket and including its rejection reason and the renewed evidence.
4. Leave the previous ticket unchanged; do not reopen it.
5. Require fresh approval before implementation.

The 90 days is a rejection cooldown, not a rolling feedback-analysis window. Supporting feedback remains evidence subject to the retention policy.

## 9. Approval and ticket changes

Approval happens in **Linear**, through an explicit **"Approved for implementation"** status or equivalent configured action.

Ship Signal verifies that the actor is a configured approver. A status change by an unauthorized actor does not authorize execution.

For MVP, the team handles requirement changes after approval manually, including changes during implementation. The team is responsible for clarifying and approving changed scope; Ship Signal must still respect the bounded implementation requirements in section 7.

Scope snapshots, versioned approval records, approval receipts, version-specific approval commands, automated scope-change detection, and automatic pause/reapproval workflows are deferred to future scope. They are not MVP requirements.

The rationale and trade-offs are recorded in [ADR-0002](../03-adr/0002-defer-versioned-approval-for-mvp.md).

## 10. Weekend execution and failure handling

The execution policy and its trade-offs are recorded in [ADR-0004](../03-adr/0004-distinguish-scheduled-recovery-retries-and-manual-execution.md).

### Schedule and queue

- Start the approved queue **Saturday at 9 a.m. in the configured timezone**. If the service returns late, automatically start eligible work while still inside the scheduled weekend window; keep the original approval deadline and Sunday cutoff.
- Determine eligibility using the **verified actual approval time**, not when its notification arrives. Tickets approved after Saturday's deadline wait until the following weekend for automatic execution; explicit manual execution is the exception below.
- Within each repository, order eligible tickets by **Linear priority, then oldest approval**.
- Run **one active implementation at a time per repository**. Different repositories may run independently. An earlier unmerged PR does not block another implementation; use a separate branch for the new work.
- For untouched tickets, the customer can leave them for the next weekend's queue or explicitly request manual execution. They do not need another approval merely because implementation never started.
- Keep each weekly cycle's scheduled times in its assigned timezone, including that timezone's daylight-saving rules. A configured timezone change takes effect **next weekly cycle**, leaving the current cycle's boundaries unchanged.

### Recovery, retries, and limits

- Automatically resume interrupted implementation during the same permitted weekend window after establishing that it is safe to continue. A crash or service restart **does not reset consumed attempt, time, or cost limits**.
- Preserve unfinished progress and report obstacles or unfinished work. Work stopped because it failed, exhausted its limits, or remained unfinished when its permitted window ended requires a **human-authorized implementation retry**.
- A person may authorize that retry **during the current weekend**. Each human-authorized retry receives a **fresh bounded attempt/time/cost budget**, while retaining the applicable execution deadline. Automatic recovery is continuation of the existing budget, not a new retry authorization.

### Sunday cutoff and grace period

- At the configurable **Sunday-evening cutoff**, stop starting new implementations through the automatic queue.
- Implementations already underway may continue within a **bounded, configurable grace period**. This extension is for ongoing implementations; starting untouched tickets after the cutoff requires explicit manual execution.
- When the grace period expires, stop remaining automatically scheduled execution, preserve progress, and report unfinished work. The grace period does not reset attempt/time/cost limits. Explicit manual execution follows its own deadline.

### Explicit manual execution

- A person may explicitly start **any approved ticket outside the scheduled weekend window**, including a ticket approved after Saturday's deadline or an untouched ticket remaining in the queue.
- Every manual execution requires an **explicit end deadline** and remains subject to bounded attempt/time/cost limits and one active implementation per repository. Approval alone does not request a manual run.
- Support **both configured Linear actions and CLI commands** for implementation retries and manual execution. Apply the same configured-approver authorization checks through both interfaces. Exact interaction syntax and CLI identity verification remain to be specified.
- Manual execution overrides scheduling, not approval or scope. Expanded scope still requires renewed human approval; the team coordinates requirement changes manually in MVP.

Linear approval records authorization; it does not immediately start coding. Falling demand does not automatically cancel already-approved work. Persist the scheduling boundaries, eligibility, progress, and budget consumption needed to preserve these rules across restarts; the concrete recovery mechanisms remain part of point 3's open implementation design.

## 11. Draft PR delivery

Successful implementation produces a **GitHub draft PR** with relevant checks passing, evidence against the approved acceptance criteria, and remaining limitations stated.

A person reviews and merges it. Opening a PR alone is not sufficient proof of successful implementation, and unfinished work must not be represented as ready.

## 12. Deployment, credentials, and retention

The **entire product runs on customer infrastructure**, including feedback analysis, ticket generation, and implementation.

For MVP, each installation uses one host, one execution service coordinating work, and a SQLite application database on persistent local storage. Multiple active execution hosts and direct sharing of the database file over a network filesystem are outside the supported deployment. Different repositories may still run independently, with one implementation at a time per repository. See [ADR-0003](../03-adr/0003-use-sqlite-for-single-host-mvp-state.md).

Customers supply credentials for external AI providers. Document the feedback and code sent to the selected provider. Installation requires a public HTTPS endpoint for Linear webhook delivery.

For implementation testing, customers provide a development/test application environment and credentials for a dedicated **application test account**. This is an application login, not a dedicated database user. Ship Signal does not need to provision a test database for MVP. Handling shared-database interference, including test data and migration conflicts, is deferred to future scope.

Ship Signal does not require or manage restricted containers, disposable execution environments, or dedicated isolated workers for MVP. Customers decide whether and how to isolate execution in their own infrastructure, including optional Docker configuration.

The rationale and trade-offs are recorded in [ADR-0001](../03-adr/0001-leave-execution-isolation-to-customers-for-mvp.md).

Only the current weekly interval contributes to demand percentages. Preserve ticket evidence and execution records under a documented retention policy; leave the customer's original database records untouched. The exact retention duration remains to be chosen.

The getting-started documentation must cover the database contract, database choice, Linear/GitHub configuration, credentials, repository requirements, timezone/schedule configuration, and public HTTPS webhook requirement.

## 13. Rust, Rig, and model independence

The agreed architecture uses:

- **Rust** for the CLI and the single-host execution service.
- **Rig** for model-provider integrations, structured outputs, and tool-using agent loops.
- **Ship Signal's persisted Rust workflow state machine** for orchestration, checkpoints, pauses, retries, and resumption, using Rig's agent-run capabilities where appropriate.
- **Ship Signal code** for deterministic demand calculations, scheduling, approval verification, integrations, and execution boundaries.

Rust and Rig replace the earlier LangGraph/LangChain choice. Ship Signal owns persistence and recovery across the full product workflow; saving an agent run alone does not establish repository ownership or confirm the outcome of an external write.

Support separate model selection for analysis and implementation. Qualify each supported model against the capabilities required by its assigned tasks; classifying feedback and implementing code have different requirements.

Use SQLite as durable writable application storage for workflow state, separate from the read-only PostgreSQL or MySQL feedback connection. The storage and single-host deployment choice is recorded in [ADR-0003](../03-adr/0003-use-sqlite-for-single-host-mvp-state.md). Make retries safe against duplicate Linear tickets and GitHub PRs, including operations that may repeat after resumption. SQLite write locking does not replace repository ownership or external-write recovery rules; the detailed mechanisms remain open.

Start with tested external-provider integrations and keep the interface extensible for local models later. Specific provider/model names have not yet been selected. Custom adapters are needed only where existing supported integrations are insufficient.

## 14. Pilot success

Run **two weekly cycles against one real product**.

Success requires at least one feature derived from genuine feedback to be approved, implemented, and merged with less human effort than manual implementation. Track ticket usefulness, PR correctness, review effort, and operating cost.

## Open implementation decisions

The following have not been settled and must not be treated as approved choices:

- Concrete initial AI provider/model names and their task-specific capability checks.
- Exact Linear label-filter rules: strict search constraints versus contextual relevance.
- Exact SQL view definitions, database-specific types, setup validation details, and concrete row-validation rules/diagnostics for PostgreSQL and MySQL. Invalid rows are excluded and reported while analysis continues.
- Feedback namespace configuration and database-specific timestamp conversion/validation mechanics; consistent input capture and restart behavior for saved weekly inputs/classifications; and historical recomputation mechanics, including exact CLI syntax/report formatting and storage of corrected results. Historical reruns read current source data for the original interval, report through the CLI, and leave existing Linear tickets unchanged. Contract versioning/compatibility policy has not been selected.
- The precise boundary used to match the "same or similar" feature for cooldown enforcement.
- Exact retention durations and numerical defaults for attempt/time/cost budgets, the Sunday cutoff, and the grace period; time/cost accounting and command shutdown mechanisms.
- Exact Linear action and CLI command syntax for retries/manual execution, including deadline input and verification of CLI actors against configured approvers.
- Handling of multiple missed analysis cycles after prolonged downtime, without changing the agreed weekly feedback intervals or counting historical submissions as current-week feedback.
- Application-state schemas and authority, repository ownership and subprocess recovery, external-operation identity and reconciliation, and SQLite configuration and backup procedures within the accepted single-host deployment.
- Workflow checkpoint boundaries and Rig agent-run persistence/resumption within the Rust execution service.

These details must preserve the settled product behavior above.

## Future scope

- Versioned approval and scope snapshots, approval receipts, and automated detection and handling of post-approval requirement changes. MVP uses basic Linear approval and manual coordination of scope changes.
- Shared development/test database interference, including conflicts affecting other users' data or shared schemas during tests and migrations. MVP relies on a customer-provided application test account and development/test environment.

Product-managed execution isolation is outside MVP. Customers may choose to configure their own Docker or other isolation; a future Ship Signal isolation feature has not been committed to.
