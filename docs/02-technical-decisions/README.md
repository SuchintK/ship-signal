# 02 — Technical decisions for consideration

This document preserves the ten findings from the technical review and the decisions made during the follow-up discussion. It is a handoff for separate decision sessions, not an implementation plan.

The repository contains design documentation and ADRs; implementation has not started. The [product design](../01-grill-session/README.md) defines the agreed behavior. [Accepted ADRs](../03-adr/README.md) preserve architectural decisions and their rationale. Recommendations below are proposals unless explicitly marked as accepted.

Points 1 and 2 have been discussed and settled for MVP. Point 3 is partially settled: Rust, Rig, SQLite, and single-host deployment are accepted; detailed concurrency and recovery mechanisms remain open. Point 4's scheduling policy is settled; numerical defaults and implementation details remain open. Points 5–10 remain open. Deferred features must not be treated as MVP requirements. No specific future implementation of those features has been approved.

## How to discuss a point

When the user asks to work through a point, guide the discussion one unresolved decision at a time:

1. Identify the existing requirements and distinguish them from new proposals.
2. Explain the current question with a concrete example, present its options, and recommend an answer with its trade-offs.
3. Wait for the user's decision before moving to the next question. Track what is settled and what remains open.
4. Once the point is settled, update the affected product design and technical-decision documentation, and assess whether the agreed decision needs an ADR.
5. Complete the ADR follow-through: if an ADR is recommended and creation is not already authorized, explain why and ask whether to create it before ending the discussion. When authorized, create the record and update its index and related links. If the user declines or defers, record that outcome and leave creation pending or declined until the user revisits it; honor existing authorization and deferrals rather than asking again.

The existing “Questions to resolve,” “Alternatives,” and “Review recommendation” sections are review notes, not a one-to-one decision guide. Reconcile them into the question-by-question discussion; the user should not need to map them together. Keep recommendations provisional until the user accepts them.

## Decision index

| Point | Topic | Current status | ADR assessment |
| --- | --- | --- | --- |
| [1](#1-approval-provenance-and-scope-versioning) | Approval provenance and scope versioning | Advanced controls deferred; basic approval retained | Accepted [ADR-0002](../03-adr/0002-defer-versioned-approval-for-mvp.md) |
| [2](#2-execution-environment-and-credential-access) | Execution environment and credential access | Isolation customer-managed; shared-database handling deferred | Accepted [ADR-0001](../03-adr/0001-leave-execution-isolation-to-customers-for-mvp.md) |
| [3](#3-durable-state-concurrency-and-external-write-recovery) | Durable state, concurrency, and external-write recovery | Rust/Rig and SQLite single-host deployment accepted; recovery details open | Storage/deployment: accepted [ADR-0003](../03-adr/0003-use-sqlite-for-single-host-mvp-state.md) |
| [4](#4-weekend-scheduling-and-recovery) | Weekend scheduling and recovery | Scheduling policy settled; defaults and implementation details open | Accepted [ADR-0004](../03-adr/0004-distinguish-scheduled-recovery-retries-and-manual-execution.md) |
| [5](#5-feedback-contract-reproducibility-and-corrections) | Feedback contract, reproducibility, and corrections | Open | ADR recommended |
| [6](#6-feature-identity-grouping-and-cooldown-matching) | Feature identity, grouping, and cooldown matching | Open | ADR worth considering |
| [7](#7-ticket-search-boundaries-and-repository-routing) | Ticket search boundaries and repository routing | Open | ADR worth considering |
| [8](#8-repository-support-and-successful-pr-delivery) | Repository support and successful PR delivery | Open | ADR worth considering |
| [9](#9-retention-and-copied-artifacts) | Retention and copied artifacts | Open | ADR worth considering |
| [10](#10-model-qualification-and-pilot-evidence) | Model qualification and pilot evidence | Open | No new ADR needed for model names or evaluation details alone |

## 1. Approval provenance and scope versioning

**Status: settled for MVP; advanced controls deferred.** See [product design section 9](../01-grill-session/README.md) and [ADR-0002](../03-adr/0002-defer-versioned-approval-for-mvp.md).

**Original concern:** an approved ticket can change before or during implementation. Reading its latest description does not by itself establish what the approver authorized. Delayed notifications can complicate capturing the approved content.

**Options discussed:** snapshot scope when approved; publish a proposed scope version and require explicit version-specific approval; or retain basic approval and coordinate scope changes manually.

**Accepted MVP decision:** approval uses the configured Linear status or equivalent configured action, and Ship Signal verifies that the actor is a configured approver. The team handles subsequent requirement changes manually and coordinates approval of expanded scope. The agent still stops and reports when implementation requires expanded scope or new infrastructure.

**Deferred:** scope snapshots, versioned approval records, approval receipts, version-specific commands, automated change detection, and automatic pause/reapproval workflows. Earlier proposals for these controls were not retained as MVP requirements.

**Trade-off:** a simpler workflow with less traceability of the exact approved ticket content. Future discussion can examine capture timing, revocation, change classification, and user interaction if versioned approval becomes necessary.

**ADR assessment:** the decision to defer these controls is recorded in accepted ADR-0002. No duplicate ADR is needed. A later change to that decision should preserve its history.

## 2. Execution environment and credential access

**Status: settled for MVP; isolation is customer-managed.** See [product design section 12](../01-grill-session/README.md) and [ADR-0001](../03-adr/0001-leave-execution-isolation-to-customers-for-mvp.md).

**Original concern:** installing dependencies, running tests, and executing generated code can affect files, credentials, services, and resources accessible to the execution environment. Customer hosting does not specify an isolation boundary.

**Options discussed:** run in the customer-selected environment; require restricted disposable containers; or require dedicated isolated VMs/workers.

**Accepted MVP decision:** Ship Signal neither provides nor mandates restricted containers, disposable environments, or isolated workers. Customers choose whether to use Docker or another isolation mechanism in their own infrastructure. The earlier recommendation to require isolation was not adopted.

For implementation testing, the customer provides a development/test application environment and a dedicated **application test account**. This means an application login, not a dedicated database user. Ship Signal does not need to provision a test database for MVP.

**Future scope:** interference in shared development/test databases, including data and migration conflicts. The application test account does not establish database or host isolation; addressing those problems is deferred. Product-managed execution isolation is outside MVP and is not a committed future feature.

**Trade-off:** simpler setup and fewer product obligations; commands have the access granted by each customer's environment.

**ADR assessment:** isolation ownership is recorded in accepted ADR-0001. The application test account and shared-database deferral are captured in requirements; no separate ADR is needed for those scope details at present.

## 3. Durable state, concurrency, and external-write recovery

**Status: partially settled for MVP; Rust, Rig, SQLite, and single-host deployment accepted.** See [product design sections 12 and 13](../01-grill-session/README.md) and [ADR-0003 for storage and deployment](../03-adr/0003-use-sqlite-for-single-host-mvp-state.md). Detailed concurrency and external-write recovery mechanisms remain open.

**Settled constraints:** use Rust for the CLI and execution service, Rig for the AI layer, a persisted Rust workflow state machine for product orchestration, durable writable application storage separate from the read-only feedback connection, duplicate-safe Linear/GitHub operations, and one implementation at a time per repository.

**Accepted framework decision:** Rust and Rig replace the earlier LangGraph/LangChain choice. Rig supplies model integrations and agent capabilities; Ship Signal's Rust workflow code owns scheduling, approval checks, durable progress, retries, and recovery. Rig agent-run persistence can support individual agent steps; its integration with the full workflow remains to be specified.

**Accepted storage and deployment decision:** use SQLite for application state on persistent local storage, with one host and one execution service coordinating each installation. Different repositories may run independently. Multiple active execution hosts, separate SQLite files coordinating the same installation, and direct access to the database file over a network filesystem are outside the supported deployment. PostgreSQL and MySQL remain supported feedback sources.

**Why it matters:** a ticket could be created just before the process loses the API response. Retrying blindly could create another ticket. A restarted worker could overlap with an executor still running. Workflow checkpoints and saved Rig agent runs do not define the application's external-write and ownership rules.

**Questions to resolve:**

- Which Rust runtime and process lifecycle will implement the single-host execution service?
- Where are workflow checkpoints saved, and how are Rig agent runs persisted and resumed within them?
- Which records are authoritative for runs, schedules, repository ownership, and external operations?
- How are uncertain API results reconciled before retrying?
- How is repository ownership enforced, including duplicate launches and subprocesses surviving a restart? If leases are used, how is takeover made safe when a lease expires?
- How is an operation's identity persisted and correlated with external results across retries, while allowing a legitimately new ticket after rejection cooldown?
- Which SQLite settings, contention handling, and backup/restore procedures will be supported?

**Alternatives considered:** SQLite with an explicit single-host constraint; PostgreSQL-backed application state and coordination; or additional queue/coordination infrastructure. SQLite was selected to simplify MVP setup and operation.

**Earlier review recommendation, not adopted as the storage choice:** favor one PostgreSQL application store. The discussion selected SQLite instead. Durable operation records, reconciliation of uncertain writes, and preventing replaced workers from publishing remain recovery concerns; their concrete mechanisms are not yet selected. SQLite serializes database writes, not whole implementations or external API calls. Validate restart and response-loss scenarios before selecting those mechanisms.

**ADR assessment:** the storage and deployment decision is recorded in accepted ADR-0003. It does not establish a completed recovery design. Keep routine schemas, locking, and retry details in implementation specifications; reassess the ADR boundary if recovery design changes the accepted deployment or operational guarantees.

## 4. Weekend scheduling and recovery

**Status: scheduling policy settled for MVP; defaults and implementation details remain open.** See [product design section 4](../01-grill-session/README.md) and [section 10](../01-grill-session/README.md) for the agreed behavior.

**Original concern:** downtime, delayed approval notifications, and resumed work could bypass the intended execution window. Pausing a workflow does not stop an active command. The original Saturday approval deadline also needed a clear relationship to explicit manual execution.

**Accepted decisions and trade-offs:**

| Decision | Agreed answer | Trade-off |
| --- | --- | --- |
| Missed schedules | Automatically catch up Friday analysis; start Saturday's queue late only while the weekend window is still open. Preserve the original schedule boundaries. | Outages need less human intervention, but late work has less time before the existing deadlines. |
| Delayed approval notifications | Use verified actual approval time for Saturday eligibility. | Timely approvals remain eligible despite delivery delays; eligibility must be verifiable after the deadline. |
| Queue ordering and untouched work | Linear priority first, then oldest approval within each repository. Untouched work can wait for next weekend or be manually started. | Reflects team priorities without a separate manual queue; lower-priority work may wait across weekends. |
| Earlier unmerged PR | Allow another implementation on a separate branch, retaining one active implementation per repository. | Review does not block throughput, but overlap and branch conflicts still need handling. |
| Interrupted implementation | Automatically resume safely within the same permitted weekend window, preserving consumed limits. | Crashes need less human intervention; recovery must establish the actual execution state before continuing. |
| Human-authorized implementation retry | Allow another attempt during the current weekend, with a fresh bounded budget and the existing deadline. | A human can unblock work promptly; repeated authorizations can increase total spending. |
| Sunday cutoff | Stop automatic admission of new implementations; allow ongoing implementations a bounded grace period, then stop and preserve progress. | Existing work can finish, but execution may extend beyond the nominal cutoff. |
| Manual execution | Allow any approved ticket to run outside the schedule, including approvals after Saturday's deadline, with an explicit end deadline. | Provides deliberate flexibility while requiring a separate human execution request. |
| Human interaction | Support both configured Linear actions and CLI commands for retries/manual execution, with the same configured-approver checks. | Both workflows are available; actor verification and action semantics must stay consistent. |
| Timezone changes | Apply changes next weekly cycle; retain the current cycle's times and follow the assigned timezone's daylight-saving rules. | Current deadlines stay predictable; configuration changes take effect later. |

**Approval boundary:** Saturday's deadline governs automatic weekend execution. Explicit manual execution is the accepted exception; it does not waive approval or permit expanded scope. Scope snapshots, versioned approval, and automated scope-change/reapproval machinery remain deferred under point 1.

**Alternatives not selected:** skip missed schedules or require human intervention for every interruption; impose a firm Sunday deadline without grace; restrict human-authorized retries to the next weekend; block all new work behind an unmerged PR; or offer execution requests only through Linear or only through the CLI. These alternatives trade flexibility and throughput for simpler coordination or stricter timing. An unrestricted extension of the automatic queue was not selected: untouched tickets after the cutoff require manual execution with an explicit deadline.

**Remaining implementation work:** numerical attempt/time/cost limits, Sunday cutoff and grace defaults; budget accounting; Linear/CLI interaction syntax and actor verification; durable schedule/eligibility records; command shutdown and safe resumption mechanisms; and handling multiple missed analysis cycles after prolonged downtime. The latter must preserve weekly feedback intervals and must not silently move historical feedback into the current week. No defaults or concrete recovery algorithms have been selected. Validate delayed notifications, restart/budget preservation, cutoff/grace expiry, manual deadlines, duplicate execution requests, and timezone transitions before implementation is considered complete.

**ADR assessment: recorded in accepted [ADR-0004](../03-adr/0004-distinguish-scheduled-recovery-retries-and-manual-execution.md).** The record preserves the enduring distinction between scheduled execution, recovery under the existing budget, human-authorized retries with fresh budgets, and manual execution with explicit deadlines. Numerical defaults, interface syntax, and concrete recovery mechanisms remain specification work. [ADR-0003](../03-adr/0003-use-sqlite-for-single-host-mvp-state.md) establishes the storage/deployment foundation; ADR-0004 adds the execution policy without replacing an earlier accepted decision.

## 5. Feedback contract, reproducibility, and corrections

**Status: open.** See [product design sections 3–5](../01-grill-session/README.md).

**Settled constraints:** PostgreSQL and MySQL expose a uniform read-only logical schema. Count only the current week's explicit feature requests from identified customers, deduplicated per customer/feature/week. Demand must be strictly greater than 25%; there is no minimum customer count.

**Why it matters:** changing source records, late arrivals, newly established identities, or different model classifications can change the result of rerunning a week even when the arithmetic is deterministic.

**Questions to resolve:** field types and nullability; stable identifiers and product boundaries; timestamp and interval semantics; edited/deleted messages; late submissions; historical identity corrections; classification provenance; and setup validation for both databases.

**Alternatives:** query live views each time; snapshot weekly inputs; or ingest incrementally with explicit correction records.

**Review recommendation, not yet accepted:** define a versioned logical contract and half-open weekly intervals, and preserve enough input/classification provenance to explain each result. Decide how historical corrections are represented without silently counting old submissions in the current week. Validate read-only permissions, identifier compatibility, and query performance against representative data in both integrations.

**ADR assessment: recommended** for the ingestion and correction strategy. Counterpoint: exact SQL definitions and field validation belong in the schema specification, not an ADR.

## 6. Feature identity, grouping, and cooldown matching

**Status: open.** See [grouping](../01-grill-session/README.md) and [cooldown rules](../01-grill-session/README.md).

**Why it matters:** “feature” currently covers an underlying need, a requested behavior, a duplicate-work match, a rejection target, and an implementation scope. Those boundaries may differ.

For example, two of ten customers requesting email and two requesting SMS yield 40% demand for the notification need, but 20% for each preference. An email-only ticket should reveal that distinction. A rejection of SMS should not automatically imply a rejection of all notification work.

**Questions to resolve:** stable identity across weeks; need versus behavior variants; uncertain matches; classification changes; and which similar requests a rejection suppresses.

**Alternatives:** exact-behavior identity, broad need-level identity, or a hierarchy of needs and variants.

**Review recommendation, not yet accepted:** distinguish underlying need, requested variant, and implementation scope. Preserve need-level counts and variant evidence, without introducing a new threshold. Test concrete grouping/rejection examples before choosing similarity rules. Retain the settled 90-day cooldown, renewed weekly qualification, and new-ticket/fresh-approval requirements.

**ADR assessment: worth considering.** Persisted feature identity can become costly to change. Counterpoint: first validate the domain distinctions; similarity weights or prompt wording alone do not justify an ADR.

## 7. Ticket search boundaries and repository routing

**Status: open.** See [product design sections 2 and 6](../01-grill-session/README.md).

**Why it matters:** strict label filters can miss duplicate work; broad searches can mix products. Code on a branch or a completed ticket does not establish that customers can use the behavior. Repository mappings may be ambiguous for work spanning multiple repositories.

**Questions to resolve:** hard search boundaries; labels as filters versus context; handling archived/rejected work; repository ownership; multi-repository tickets; and evidence required for “already exists.”

**Alternatives:** hard label filters, contextual ranking, or a configurable combination; single-repository tickets versus coordinated multi-repository implementation.

**Review recommendation, not yet accepted:** use configured product/team/project boundaries as hard constraints and labels as context by default. Record repository/branch and relevant deployment evidence. Require clear repository ownership before implementation, and consider a single-repository-ticket limit for the pilot. Preserve rejection precedence and send uncertain matches to human review.

**Validation:** use representative existing tickets and repository mappings to check missed duplicates, unrelated matches, and ambiguous ownership.

**ADR assessment: worth considering** for durable routing and cross-repository boundaries. Counterpoint: label weights and ordinary search settings can remain specification/configuration details.

## 8. Repository support and successful PR delivery

**Status: open.** See [bounded scope](../01-grill-session/README.md) and [draft PR delivery](../01-grill-session/README.md).

**Settled constraints:** relevant checks pass, acceptance-criteria evidence is supplied, limitations are stated, and a human reviews/merges the draft PR. New code, tables, and migrations can qualify within bounded scope.

**Why it matters:** “relevant checks” is not yet an executable contract. An inferred test command may provide insufficient evidence. Some repositories need a PR opened before their checks run.

**Questions to resolve:** supported runtimes/toolchains; setup and test commands; required checks; baseline failures; unavailable dependencies; migration verification; branch/base-commit handling; and pending versus successful delivery states.

**Alternatives:** infer commands per repository, require a verification manifest, or support a small set of documented conventions.

**Review recommendation, not yet accepted:** require explicit onboarding configuration for setup, tests, required checks, and test services. Separate provisional draft creation from verified delivery; missing required checks do not count as passing. Validate against a real pilot repository. Testing uses the customer-provided environment/account agreed in point 2; mandatory isolated workers, disposable databases, and shared-database conflict handling are not MVP prerequisites.

**ADR assessment: worth considering** for a durable repository support/execution contract. Counterpoint: concrete commands and acceptance checks usually belong in onboarding specifications.

## 9. Retention and copied artifacts

**Status: open.** See [product design section 12](../01-grill-session/README.md).

**Settled constraints:** customer-hosted operation; external AI providers with customer credentials; documented feedback/code disclosure; preserved evidence and execution records under a retention policy; original feedback records untouched.

**Why it matters:** evidence may be copied into checkpoints, logs, Linear, GitHub, and provider requests. Deleting a local copy does not remove other copies. Rejection and approval metadata may need different lifetimes from raw feedback.

**Questions to resolve:** artifact inventory; retention durations; redaction; deletion responsibilities across destinations; data supplied to providers; and interaction between deletion, cooldown enforcement, and leaving rejected tickets unchanged.

**Alternatives:** retain full evidence copies, references only, or minimized excerpts with durable metadata.

**Review recommendation, not yet accepted:** inventory destinations and define separate policies for raw feedback, workflow records, rejection metadata, and external copies. Favor minimized excerpts in tickets/PRs and retain enough rejection identity to enforce cooldowns. Validate the policy against actual data flows and deletion capabilities before selecting durations.

**ADR assessment: worth considering** for durable data ownership/disclosure/deletion policy. Counterpoint: numerical retention defaults alone are configuration decisions, and a data-policy specification may capture the detail more usefully.

## 10. Model qualification and pilot evidence

**Status: open.** See [model support](../01-grill-session/README.md) and [pilot success](../01-grill-session/README.md).

**Settled constraints:** Rust workflow orchestration with Rig model integrations and agent capabilities, separate analysis/implementation model selection, tested external-provider integrations initially, extensibility for local models later, and two weekly cycles against one real product.

Pilot success requires a genuine-feedback feature to be approved, implemented, and merged with less human effort than manual implementation. Ticket usefulness and implementation reliability are evaluated separately.

**Why it matters:** provider compatibility does not establish task reliability. Two cycles may contain no qualifying bounded feature. “Less human effort” lacks a measurement baseline.

**Questions to resolve:** initial provider/model choices; task capabilities; representative evaluation cases; model/prompt/configuration provenance; cost budgets; manual-effort baseline; and interpretation of a pilot with no eligible work.

**Alternatives:** informal evaluation after model selection, or representative task evaluations before selecting an initial model combination.

**Review recommendation, not yet accepted:** evaluate analysis and implementation separately, including grouping errors, duplicate matching, bounded-scope coding, checks, recovery, and cost. Track active human time across investigation, approval, review, and repair, reporting setup separately. Assess whether the pilot product supplies enough genuine eligible feedback; do not change the threshold or introduce a minimum customer count implicitly.

**ADR assessment: no new ADR needed** for model names or evaluation metrics alone. Reassess if a choice changes provider/data boundaries, runtime responsibilities, or foundational dependencies. Model selection should work within the agreed Rust/Rig architecture.

## Discussion dependencies and source checks

Point 3 informs scheduling recovery in point 4. Points 5 and 6 establish the evidence and identity semantics needed by point 7. Points 2 and 8 constrain implementation verification. Point 9 applies across all stored/copied artifacts. Point 10 evaluates the resulting workflow.

The initial review found no unambiguous contradiction among the settled product rules. It identified gaps in precedence and semantics, especially weekend recovery, broad demand versus narrow implementation scope, evidence retention/deletion, and PR creation/check sequencing. Automated post-approval scope enforcement was subsequently deferred by the decision in point 1.

The review consulted these primary sources; integration capabilities should be rechecked when their implementation is discussed:

- [Linear webhooks](https://linear.app/developers/webhooks): issue/comment updates, actor information, authentication, and delivery behavior.
- [Rig workflows](https://rig.rs/docs/concepts/chains): workflow composition in Rust for the accepted framework choice.
- [Rig durable runs](https://rig.rs/docs/concepts/runs): agent-run serialization, pauses, and resumption; application storage and recovery still need explicit design.
- [Rig providers and clients](https://rig.rs/docs/concepts/provider_clients): model-provider integrations.
- [LangGraph persistence](https://docs.langchain.com/oss/python/langgraph/persistence): a source consulted for the earlier LangGraph design, replaced by Rust/Rig.
- [GitHub secure use reference](https://docs.github.com/en/actions/reference/security/secure-use): risks of executing untrusted code and handling credentials.
- [Docker Engine security](https://docs.docker.com/engine/security/): execution isolation and configuration trade-offs considered before point 2 was settled.

An open point does not automatically require an ADR. Discuss the alternatives and validate material assumptions first; use a specification for implementation details and an ADR for a consequential agreed decision and its rationale.
