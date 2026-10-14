# 02 — Technical decisions for consideration

This document preserves the ten findings from the technical review and the decisions made during the follow-up discussion. It is a handoff for separate decision sessions, not an implementation plan.

The repository contains design documentation and ADRs; implementation has not started. The [product design](../01-grill-session/README.md) defines the agreed behavior. [Accepted ADRs](../03-adr/README.md) preserve architectural decisions and their rationale. Recommendations below are proposals unless explicitly marked as accepted.

Points 1 and 2 have been discussed and settled for MVP. Points 3–10 remain open. Deferred features must not be treated as MVP requirements. No specific future implementation of those features has been approved.

## Decision index

| Point | Topic | Current status | ADR assessment |
| --- | --- | --- | --- |
| [1](#1-approval-provenance-and-scope-versioning) | Approval provenance and scope versioning | Advanced controls deferred; basic approval retained | Accepted [ADR-0002](../03-adr/0002-defer-versioned-approval-for-mvp.md) |
| [2](#2-execution-environment-and-credential-access) | Execution environment and credential access | Isolation customer-managed; shared-database handling deferred | Accepted [ADR-0001](../03-adr/0001-leave-execution-isolation-to-customers-for-mvp.md) |
| [3](#3-durable-state-concurrency-and-external-write-recovery) | Durable state, concurrency, and external-write recovery | Open | ADR recommended |
| [4](#4-weekend-scheduling-and-recovery) | Weekend scheduling and recovery | Open | ADR worth considering |
| [5](#5-feedback-contract-reproducibility-and-corrections) | Feedback contract, reproducibility, and corrections | Open | ADR recommended |
| [6](#6-feature-identity-grouping-and-cooldown-matching) | Feature identity, grouping, and cooldown matching | Open | ADR worth considering |
| [7](#7-ticket-search-boundaries-and-repository-routing) | Ticket search boundaries and repository routing | Open | ADR worth considering |
| [8](#8-repository-support-and-successful-pr-delivery) | Repository support and successful PR delivery | Open | ADR worth considering |
| [9](#9-retention-and-copied-artifacts) | Retention and copied artifacts | Open | ADR worth considering |
| [10](#10-model-qualification-and-pilot-evidence) | Model qualification and pilot evidence | Open | No new ADR needed for model names or evaluation details alone |

## 1. Approval provenance and scope versioning

**Status: settled for MVP; advanced controls deferred.** See [product design section 9](../01-grill-session/README.md#9-approval-and-ticket-changes) and [ADR-0002](../03-adr/0002-defer-versioned-approval-for-mvp.md).

**Original concern:** an approved ticket can change before or during implementation. Reading its latest description does not by itself establish what the approver authorized. Delayed notifications can complicate capturing the approved content.

**Options discussed:** snapshot scope when approved; publish a proposed scope version and require explicit version-specific approval; or retain basic approval and coordinate scope changes manually.

**Accepted MVP decision:** approval uses the configured Linear status or equivalent configured action, and Ship Signal verifies that the actor is a configured approver. The team handles subsequent requirement changes manually and coordinates approval of expanded scope. The agent still stops and reports when implementation requires expanded scope or new infrastructure.

**Deferred:** scope snapshots, versioned approval records, approval receipts, version-specific commands, automated change detection, and automatic pause/reapproval workflows. Earlier proposals for these controls were not retained as MVP requirements.

**Trade-off:** a simpler workflow with less traceability of the exact approved ticket content. Future discussion can examine capture timing, revocation, change classification, and user interaction if versioned approval becomes necessary.

**ADR assessment:** the decision to defer these controls is recorded in accepted ADR-0002. No duplicate ADR is needed. A later change to that decision should preserve its history.

## 2. Execution environment and credential access

**Status: settled for MVP; isolation is customer-managed.** See [product design section 12](../01-grill-session/README.md#12-deployment-credentials-and-retention) and [ADR-0001](../03-adr/0001-leave-execution-isolation-to-customers-for-mvp.md).

**Original concern:** installing dependencies, running tests, and executing generated code can affect files, credentials, services, and resources accessible to the execution environment. Customer hosting does not specify an isolation boundary.

**Options discussed:** run in the customer-selected environment; require restricted disposable containers; or require dedicated isolated VMs/workers.

**Accepted MVP decision:** Ship Signal neither provides nor mandates restricted containers, disposable environments, or isolated workers. Customers choose whether to use Docker or another isolation mechanism in their own infrastructure. The earlier recommendation to require isolation was not adopted.

For implementation testing, the customer provides a development/test application environment and a dedicated **application test account**. This means an application login, not a dedicated database user. Ship Signal does not need to provision a test database for MVP.

**Future scope:** interference in shared development/test databases, including data and migration conflicts. The application test account does not establish database or host isolation; addressing those problems is deferred. Product-managed execution isolation is outside MVP and is not a committed future feature.

**Trade-off:** simpler setup and fewer product obligations; commands have the access granted by each customer's environment.

**ADR assessment:** isolation ownership is recorded in accepted ADR-0001. The application test account and shared-database deferral are captured in requirements; no separate ADR is needed for those scope details at present.

## 3. Durable state, concurrency, and external-write recovery

**Status: open.** See [product design section 13](../01-grill-session/README.md#13-langgraph-and-model-independence).

**Settled constraints:** use LangGraph, durable writable application storage separate from the read-only feedback connection, duplicate-safe Linear/GitHub operations, and one implementation at a time per repository.

**Why it matters:** a ticket could be created just before the process loses the API response. Retrying blindly could create another ticket. A restarted worker could overlap with an executor still running. Graph checkpoints do not define the application's external-write and ownership rules.

**Questions to resolve:**

- Which application storage engine and runtime will be supported initially?
- Which records are authoritative for runs, schedules, repository ownership, and external operations?
- How are uncertain API results reconciled before retrying?
- How is concurrent execution prevented, including after a restart or expired worker lease?
- What identifies an operation across retries, while allowing a legitimately new ticket after rejection cooldown?

**Alternatives:** SQLite with an explicit single-host constraint; PostgreSQL-backed application state and coordination; or additional queue/coordination infrastructure.

**Review recommendation, not yet accepted:** favor one PostgreSQL application store, persist operation intents/results, reconcile uncertain writes, and prevent replaced workers from publishing. Supporting MySQL feedback does not require MySQL application-state storage. Validate restart and response-loss scenarios before selecting the mechanism.

**ADR assessment: recommended.** Storage and recovery choices shape durable state and operational guarantees. Counterpoint: avoid extra infrastructure or separate ADRs for each retry detail; record one coherent persistence/recovery strategy.

## 4. Weekend scheduling and recovery

**Status: open.** See [product design section 10](../01-grill-session/README.md#10-weekend-execution-and-failure-handling).

**Settled constraints:** Friday analysis at 9 a.m.; Saturday queue start at 9 a.m.; customer-configured timezone; approvals after Saturday's start wait until the next weekend; configurable Sunday-evening cutoff; bounded attempts; unfinished progress preserved; human decision to retry.

**Why it matters:** downtime, delayed approval notifications, and resumed work can otherwise bypass the intended execution window. A workflow pause alone does not define how an active command stops at the cutoff.

**Questions to resolve:** missed schedules and catch-up; daylight-saving/timezone changes; queue ordering; eligible work when a previous PR remains unmerged; recovery of unchanged work versus a human-requested retry; manual reapproval after the cutoff; and attempt/time/cost limits.

**Alternatives:** a fixed weekend cohort; controlled admission during the window; or explicit exception actions. Any exception to the settled Saturday approval cutoff would require an agreed requirement change.

**Review recommendation, not yet accepted:** preserve the Saturday cutoff, persist scheduling boundaries and eligible queue membership, distinguish recovery from retry, and bound active commands so the Sunday cutoff is enforceable. Validate downtime and cutoff scenarios. Do not reintroduce the scope-version or automated reapproval machinery deferred in point 1.

**ADR assessment: worth considering.** A durable execution policy can deserve a record; numerical defaults and routine scheduling details belong in configuration/specifications. Recovery mechanisms may already be covered by point 3's eventual ADR.

## 5. Feedback contract, reproducibility, and corrections

**Status: open.** See [product design sections 3–5](../01-grill-session/README.md#3-uniform-feedback-schema).

**Settled constraints:** PostgreSQL and MySQL expose a uniform read-only logical schema. Count only the current week's explicit feature requests from identified customers, deduplicated per customer/feature/week. Demand must be strictly greater than 25%; there is no minimum customer count.

**Why it matters:** changing source records, late arrivals, newly established identities, or different model classifications can change the result of rerunning a week even when the arithmetic is deterministic.

**Questions to resolve:** field types and nullability; stable identifiers and product boundaries; timestamp and interval semantics; edited/deleted messages; late submissions; historical identity corrections; classification provenance; and setup validation for both databases.

**Alternatives:** query live views each time; snapshot weekly inputs; or ingest incrementally with explicit correction records.

**Review recommendation, not yet accepted:** define a versioned logical contract and half-open weekly intervals, and preserve enough input/classification provenance to explain each result. Decide how historical corrections are represented without silently counting old submissions in the current week. Validate read-only permissions, identifier compatibility, and query performance against representative data in both integrations.

**ADR assessment: recommended** for the ingestion and correction strategy. Counterpoint: exact SQL definitions and field validation belong in the schema specification, not an ADR.

## 6. Feature identity, grouping, and cooldown matching

**Status: open.** See [grouping](../01-grill-session/README.md#5-customer-identity-deduplication-and-grouping) and [cooldown rules](../01-grill-session/README.md#8-ticket-eligibility-and-rejected-feature-cooldown).

**Why it matters:** “feature” currently covers an underlying need, a requested behavior, a duplicate-work match, a rejection target, and an implementation scope. Those boundaries may differ.

For example, two of ten customers requesting email and two requesting SMS yield 40% demand for the notification need, but 20% for each preference. An email-only ticket should reveal that distinction. A rejection of SMS should not automatically imply a rejection of all notification work.

**Questions to resolve:** stable identity across weeks; need versus behavior variants; uncertain matches; classification changes; and which similar requests a rejection suppresses.

**Alternatives:** exact-behavior identity, broad need-level identity, or a hierarchy of needs and variants.

**Review recommendation, not yet accepted:** distinguish underlying need, requested variant, and implementation scope. Preserve need-level counts and variant evidence, without introducing a new threshold. Test concrete grouping/rejection examples before choosing similarity rules. Retain the settled 90-day cooldown, renewed weekly qualification, and new-ticket/fresh-approval requirements.

**ADR assessment: worth considering.** Persisted feature identity can become costly to change. Counterpoint: first validate the domain distinctions; similarity weights or prompt wording alone do not justify an ADR.

## 7. Ticket search boundaries and repository routing

**Status: open.** See [product design sections 2 and 6](../01-grill-session/README.md#6-investigating-existing-tickets-and-code).

**Why it matters:** strict label filters can miss duplicate work; broad searches can mix products. Code on a branch or a completed ticket does not establish that customers can use the behavior. Repository mappings may be ambiguous for work spanning multiple repositories.

**Questions to resolve:** hard search boundaries; labels as filters versus context; handling archived/rejected work; repository ownership; multi-repository tickets; and evidence required for “already exists.”

**Alternatives:** hard label filters, contextual ranking, or a configurable combination; single-repository tickets versus coordinated multi-repository implementation.

**Review recommendation, not yet accepted:** use configured product/team/project boundaries as hard constraints and labels as context by default. Record repository/branch and relevant deployment evidence. Require clear repository ownership before implementation, and consider a single-repository-ticket limit for the pilot. Preserve rejection precedence and send uncertain matches to human review.

**Validation:** use representative existing tickets and repository mappings to check missed duplicates, unrelated matches, and ambiguous ownership.

**ADR assessment: worth considering** for durable routing and cross-repository boundaries. Counterpoint: label weights and ordinary search settings can remain specification/configuration details.

## 8. Repository support and successful PR delivery

**Status: open.** See [bounded scope](../01-grill-session/README.md#7-bounded-implementation-scope) and [draft PR delivery](../01-grill-session/README.md#11-draft-pr-delivery).

**Settled constraints:** relevant checks pass, acceptance-criteria evidence is supplied, limitations are stated, and a human reviews/merges the draft PR. New code, tables, and migrations can qualify within bounded scope.

**Why it matters:** “relevant checks” is not yet an executable contract. An inferred test command may provide insufficient evidence. Some repositories need a PR opened before their checks run.

**Questions to resolve:** supported runtimes/toolchains; setup and test commands; required checks; baseline failures; unavailable dependencies; migration verification; branch/base-commit handling; and pending versus successful delivery states.

**Alternatives:** infer commands per repository, require a verification manifest, or support a small set of documented conventions.

**Review recommendation, not yet accepted:** require explicit onboarding configuration for setup, tests, required checks, and test services. Separate provisional draft creation from verified delivery; missing required checks do not count as passing. Validate against a real pilot repository. Testing uses the customer-provided environment/account agreed in point 2; mandatory isolated workers, disposable databases, and shared-database conflict handling are not MVP prerequisites.

**ADR assessment: worth considering** for a durable repository support/execution contract. Counterpoint: concrete commands and acceptance checks usually belong in onboarding specifications.

## 9. Retention and copied artifacts

**Status: open.** See [product design section 12](../01-grill-session/README.md#12-deployment-credentials-and-retention).

**Settled constraints:** customer-hosted operation; external AI providers with customer credentials; documented feedback/code disclosure; preserved evidence and execution records under a retention policy; original feedback records untouched.

**Why it matters:** evidence may be copied into checkpoints, logs, Linear, GitHub, and provider requests. Deleting a local copy does not remove other copies. Rejection and approval metadata may need different lifetimes from raw feedback.

**Questions to resolve:** artifact inventory; retention durations; redaction; deletion responsibilities across destinations; data supplied to providers; and interaction between deletion, cooldown enforcement, and leaving rejected tickets unchanged.

**Alternatives:** retain full evidence copies, references only, or minimized excerpts with durable metadata.

**Review recommendation, not yet accepted:** inventory destinations and define separate policies for raw feedback, workflow records, rejection metadata, and external copies. Favor minimized excerpts in tickets/PRs and retain enough rejection identity to enforce cooldowns. Validate the policy against actual data flows and deletion capabilities before selecting durations.

**ADR assessment: worth considering** for durable data ownership/disclosure/deletion policy. Counterpoint: numerical retention defaults alone are configuration decisions, and a data-policy specification may capture the detail more usefully.

## 10. Model qualification and pilot evidence

**Status: open.** See [model support](../01-grill-session/README.md#13-langgraph-and-model-independence) and [pilot success](../01-grill-session/README.md#14-pilot-success).

**Settled constraints:** LangGraph orchestration, LangChain model integrations, separate analysis/implementation model selection, tested external-provider integrations initially, extensibility for local models later, and two weekly cycles against one real product.

Pilot success requires a genuine-feedback feature to be approved, implemented, and merged with less human effort than manual implementation. Ticket usefulness and implementation reliability are evaluated separately.

**Why it matters:** provider compatibility does not establish task reliability. Two cycles may contain no qualifying bounded feature. “Less human effort” lacks a measurement baseline.

**Questions to resolve:** initial provider/model choices; task capabilities; representative evaluation cases; model/prompt/configuration provenance; cost budgets; manual-effort baseline; and interpretation of a pilot with no eligible work.

**Alternatives:** informal evaluation after model selection, or representative task evaluations before selecting an initial model combination.

**Review recommendation, not yet accepted:** evaluate analysis and implementation separately, including grouping errors, duplicate matching, bounded-scope coding, checks, recovery, and cost. Track active human time across investigation, approval, review, and repair, reporting setup separately. Assess whether the pilot product supplies enough genuine eligible feedback; do not change the threshold or introduce a minimum customer count implicitly.

**ADR assessment: no new ADR needed** for model names or evaluation metrics alone. Reassess if a choice changes provider/data boundaries, runtime responsibilities, or foundational dependencies. Do not reopen the settled LangGraph choice merely to select a model.

## Discussion dependencies and source checks

Point 3 informs scheduling recovery in point 4. Points 5 and 6 establish the evidence and identity semantics needed by point 7. Points 2 and 8 constrain implementation verification. Point 9 applies across all stored/copied artifacts. Point 10 evaluates the resulting workflow.

The initial review found no unambiguous contradiction among the settled product rules. It identified gaps in precedence and semantics, especially weekend recovery, broad demand versus narrow implementation scope, evidence retention/deletion, and PR creation/check sequencing. Automated post-approval scope enforcement was subsequently deferred by the decision in point 1.

The review consulted these primary sources; integration capabilities should be rechecked when their implementation is discussed:

- [Linear webhooks](https://linear.app/developers/webhooks): issue/comment updates, actor information, authentication, and delivery behavior.
- [LangGraph persistence](https://docs.langchain.com/oss/python/langgraph/persistence): checkpointed graph state and application-defined state.
- [GitHub secure use reference](https://docs.github.com/en/actions/reference/security/secure-use): risks of executing untrusted code and handling credentials.
- [Docker Engine security](https://docs.docker.com/engine/security/): execution isolation and configuration trade-offs considered before point 2 was settled.

An open point does not automatically require an ADR. Discuss the alternatives and validate material assumptions first; use a specification for implementation details and an ADR for a consequential agreed decision and its rationale.
