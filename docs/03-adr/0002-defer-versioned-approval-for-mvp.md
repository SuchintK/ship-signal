# ADR-0002: Defer versioned approval for MVP

- **Status:** Accepted
- **Date:** 2026-10-14

## Context

Ship Signal requires human approval in Linear before implementing a ticket. The initial product design also required saving the approved scope and automatically pausing or requesting renewed approval when requirements materially changed.

The architecture review explored scope versions, approval receipts, and version-specific approval commands. These controls and the associated workflow were judged unnecessary for MVP. The team will coordinate post-approval requirement changes manually instead.

## Decision

For MVP, approval uses the configured Linear approval status or equivalent configured action. Ship Signal verifies that the actor is a configured approver; an unauthorized status change does not authorize execution.

Defer scope snapshots, versioned approval records, approval receipts, version-specific approval commands, automated scope-change detection, and automatic pause/reapproval workflows.

The team handles requirement changes after approval, including during implementation, and coordinates renewed human approval when scope expands. Ship Signal still follows the bounded implementation requirements: stop and report when implementation requires expanded scope or new infrastructure.

Versioned approval may be considered in future scope. Its interaction design and implementation are not settled by this ADR.

## Alternatives considered

- **Snapshot scope on approval and automatically evaluate later changes:** improves traceability and can detect unauthorized scope expansion, but requires additional persistence, change interpretation, and pause/reapproval behavior.
- **Require explicit approval of a published scope version:** makes the approved revision explicit, but adds version-specific actions to the approver's workflow. This interaction was rejected for MVP as overkill.

## Consequences

- Approvers can use the basic Linear workflow without managing scope versions or approval commands.
- MVP avoids implementing approval snapshots and automated interpretation of requirement changes.
- Ship Signal has no versioned scope record establishing exactly which ticket text was approved. Post-approval edits are not automatically detected or handled by an approval-enforcement workflow.
- Reliable coordination of requirement changes depends on the team. This trade-off does not remove the configured-approver check or the requirement to obtain human approval for expanded scope.
- Future versioned approval would require a separate decision about scope capture, change handling, and the approval interaction.

## References

- [Product design: approval and ticket changes](../01-grill-session/README.md#9-approval-and-ticket-changes)
- [Product design: bounded implementation scope](../01-grill-session/README.md#7-bounded-implementation-scope)
