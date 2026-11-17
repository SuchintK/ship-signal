---
name: scope-impact
description: Assess downstream impact when Ship Signal development work changes scope, behavior, assumptions, or acceptance criteria. Use proactively when such a change is requested or discovered during implementation.
---

# Scope impact

When a change affects Ship Signal development work, determine what else may need to change:

1. Identify the affected Linear issue, read its description and discussion, and understand the scope change. If no issue can be identified, check the available context and say what could not be checked.
2. Follow downstream blocking relationships and inspect further dependencies where the impact carries forward. Also check relevant linked work, the shared specification, and applicable ADRs; a dependency link alone does not establish impact.
3. Tell the user which tickets or documents may need changes and why. Flag implications for acceptance criteria, dependencies, readiness, and conflicts with accepted ADRs. Suggest concrete updates, or report what was checked if none appear necessary.
4. Apply updates within the user's authorized scope, preserving unrelated issue fields and labels. Distinguish accepted decisions from proposals and keep ADR history intact.

This workflow assesses Ship Signal's own development work. It does not monitor Linear edits or alter the customer-facing approval workflow.
