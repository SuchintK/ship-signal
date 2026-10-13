# 01 — Grill session: settled product design

This record consolidates the decisions agreed during the grill session. It includes the later changes to database support, ticket eligibility, rejected-feature cooldowns, model independence, and approval semantics. Earlier alternatives are not current requirements.

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

The contract covers information such as message ID, customer ID, submission time, original feedback text, and source reference. Exact SQL definitions and field types remain to be specified.

Document the contract in the README/getting-started guide and validate it during setup. The feedback connection must not modify the customer's original records.

## 4. Weekly analysis and demand threshold

Run analysis **Friday at 9 a.m. in a customer-configured timezone**, covering the interval from the previous Friday's cutoff to the current Friday's cutoff.

Calculate:

**Demand percentage = distinct customers requesting a particular feature ÷ distinct customers requesting any feature that week × 100.**

A feature qualifies for consideration only when demand is **strictly greater than 25%**. Exactly 25% does not qualify. There is no additional minimum customer count; show actual counts alongside percentages so the evidence is visible.

The denominator is feature-requesting customers, not all registered customers and not all feedback senders. Bug-only customers are excluded. A customer who submits both bugs and feature requests still counts once in the denominator.

Only that week's feedback contributes to the calculation. The rejected-feature cooldown described below does not change this counting window.

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

Ship Signal verifies that the actor is a configured approver and saves a snapshot of the scope and ticket content being approved. A status change by an unauthorized actor does not authorize execution.

| Timing or type of change | Behavior |
| --- | --- |
| Additions before approval | Include them in the latest scope being approved and implemented, subject to supported scope. |
| Material changes after approval but before coding | Require renewed approval before implementation starts. |
| Material changes during implementation | Pause execution, preserve progress, and request approval of the revised scope. |
| Typos, formatting, or background information that does not change requirements | No renewed approval is required. |

For example, adding email delivery to the description before approval makes it part of the proposed work. Adding multiple recipients after approval expands authorized behavior and requires renewed approval.

Approval authorizes a specific scope, not an indefinitely changing description. Ambiguous changes should not silently become implementation instructions.

## 10. Weekend execution and failure handling

- Start the approved queue **Saturday at 9 a.m. in the configured timezone**.
- Tickets approved after that start wait until the following weekend.
- Run one implementation at a time per repository. Different repositories may run independently.
- Use bounded attempts within the approved scope.
- Stop cleanly at a configurable **Sunday-evening cutoff**.
- Preserve unfinished progress and report obstacles or unfinished work. A person decides whether to retry.
- Expanded scope requires renewed approval.

Linear approval records authorization; it does not immediately start coding on Friday. Falling demand does not automatically cancel already-approved work.

## 11. Draft PR delivery

Successful implementation produces a **GitHub draft PR** with relevant checks passing, evidence against the approved acceptance criteria, and remaining limitations stated.

A person reviews and merges it. Opening a PR alone is not sufficient proof of successful implementation, and unfinished work must not be represented as ready.

## 12. Deployment, credentials, and retention

The **entire product runs on customer infrastructure**, including feedback analysis, ticket generation, and implementation.

Customers supply credentials for external AI providers. Document the feedback and code sent to the selected provider. Installation requires a public HTTPS endpoint for Linear webhook delivery.

Only the current weekly interval contributes to demand percentages. Preserve ticket evidence and execution records under a documented retention policy; leave the customer's original database records untouched. The exact retention duration remains to be chosen.

The getting-started documentation must cover the database contract, database choice, Linear/GitHub configuration, credentials, repository requirements, timezone/schedule configuration, and public HTTPS webhook requirement.

## 13. LangGraph and model independence

The agreed architecture uses:

- **LangGraph** for orchestration, checkpoints, pauses, retries, and resumption.
- **LangChain model integrations** for access to supported model providers.
- **Ship Signal code** for deterministic demand calculations, scheduling, approval verification, integrations, and execution boundaries.

Support separate model selection for analysis and implementation. Qualify each supported model against the capabilities required by its assigned tasks; classifying feedback and implementing code have different requirements.

Use durable writable application storage for workflow state, separate from the read-only feedback connection. Make retries safe against duplicate Linear tickets and GitHub PRs, including operations that may repeat after resumption.

Start with tested external-provider integrations and keep the interface extensible for local models later. Specific provider/model names have not yet been selected. Custom adapters are needed only where existing supported integrations are insufficient.

## 14. Pilot success

Run **two weekly cycles against one real product**.

Success requires at least one feature derived from genuine feedback to be approved, implemented, and merged with less human effort than manual implementation. Track ticket usefulness, PR correctness, review effort, and operating cost.

## Open implementation decisions

The following have not been settled and must not be treated as approved choices:

- Concrete initial AI provider/model names and their task-specific capability checks.
- Exact Linear label-filter rules: strict search constraints versus contextual relevance.
- Exact SQL view definitions, field types, and setup validation details for PostgreSQL and MySQL.
- The precise boundary used to match the "same or similar" feature for cooldown enforcement.
- Exact retention durations and numerical defaults for bounded attempts and the Sunday cutoff.

These details must preserve the settled product behavior above.
