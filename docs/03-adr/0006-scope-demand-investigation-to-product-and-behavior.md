# ADR-0006: Scope demand investigation to product and behavior

- **Status:** Accepted
- **Date:** 2026-11-19

## Context

Weekly demand groups requests by the customer’s underlying need. Requests for email and SMS alerts can together exceed 25% while asking for different behavior. Treating the need as ticket scope would add evidence to tickets that cover a different variant and could suppress unrelated work after a rejection. Shared Linear teams and GitHub repositories add another ambiguity when configuration does not establish which product or repository owns a change.

## Decision

Use a product-scoped need ID for analysis. Qualify demand at the need level and retain requested variants. Match existing work against the specific requested behavior and ticket scope. Use evidence about the rejected behavior as the input to cooldown decisions. SUC-22 will define and enforce the similarity boundary and expiry rules.

Search explicit Linear team and project pairs owned by the selected product. A team’s unprojected issues are configured as their own scope. Keep the customer’s labels as investigation context and include unlabeled issues in the search.

Require one repository with evidence of ownership for each implementation proposal. Route ambiguous or coordinated multi-repository changes for human review. Verify existing behavior with code from a pinned revision of a customer-designated live ref.

## Alternatives considered

- **Use each variant as its own demand identity.** This makes qualification easier to explain for individual behaviors, but discards shared demand when several variants express the same need.
- **Use the need as both demand identity and ticket scope.** This simplifies matching, but can conflate different changes and incorrectly carry approvals or rejection decisions across variants.
- **Require a matching label or choose a default repository.** Both approaches simplify routing. They can miss existing work or assign changes without evidence of ownership.

## Consequences

A proposal may cover one variant of a qualifying need. Its evidence must distinguish overall need demand from the scope of the proposed change. Evidence updates preserve the existing ticket’s scope and approval.

Customer teams maintain the Linear scopes, repository routes, and live refs. Proposal generation stops at human review when those inputs or the available evidence cannot establish ownership or scope. SUC-24 and SUC-25 consume a proposal with one repository. SUC-31 uses the product-scoped Linear write intent to reconcile uncertain writes.

## References

- [SUC-21](https://linear.app/suchint/issue/SUC-21/ship-signal-investigate-demand-and-propose-linear-work)
- [SUC-22](https://linear.app/suchint/issue/SUC-22/ship-signal-suppress-and-reconsider-rejected-work)
- [Product design](../01-grill-session/README.md#6-investigating-existing-tickets-and-code)
- [ADR-0002](0002-defer-versioned-approval-for-mvp.md)
- [ADR-0005](0005-preserve-weekly-feedback-evidence-and-require-explicit-historical-corrections.md)
