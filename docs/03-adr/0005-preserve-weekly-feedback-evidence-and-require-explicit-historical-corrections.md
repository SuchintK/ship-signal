# ADR-0005: Preserve weekly feedback evidence and require explicit historical corrections

- **Status:** Accepted
- **Date:** 2026-10-20

## Context

Ship Signal uses weekly customer demand to support Linear tickets. Source messages, customer identities, and model classifications can change after analysis. Requerying live feedback or rerunning a classifier cannot reliably explain the evidence behind an earlier result.

MVP needs stable evidence without continuous change ingestion, automatic historical corrections, or extensive model-run provenance tracking. The customer feedback connection remains read-only; application state is stored separately under ADR-0003.

## Decision

Save the feedback inputs and classification results used by each weekly analysis. Explain completed demand calculations using those saved inputs and classifications. Later source edits or classification changes must not silently rewrite an earlier result. Weekly intervals use the accepted submission-time, timezone, identifier, and field-contract rules in the product design.

Historical corrections require an explicit CLI request to rerun the affected week. Reread the current feedback database for the original interval, retaining its original cutoffs and assigned timezone. Analyze current message text and customer identities, include late-arriving submissions within that interval, and omit messages no longer exposed by the source, including deleted messages. Save corrected inputs and classifications separately from the original result. Historical submissions must not enter current-week demand merely because a correction happened now.

Display the corrected report in the CLI, showing the week, previous and corrected customer counts and percentages, invalid-row exclusion counts/reasons, and existing ticket references where available. Leave existing Linear tickets unchanged; people decide whether to update their evidence or demand numbers. Falling demand does not invalidate an existing ticket.

Exclude invalid feedback rows, report their count and validation reasons, and continue analysis on valid rows under the normal demand threshold and ticket eligibility rules. A null customer ID remains valid unidentified feedback, preserved but excluded from percentages.

Additional per-result tracking of provider/model identifiers, prompts, settings, software/contract versions, and input-capture metadata is not required for MVP. Saved inputs, classifications, and exclusion diagnostics remain required; exact reproduction of model runs is not guaranteed.

## Alternatives considered

- **Always requery live views:** reduces copied data, but source changes can obscure why an earlier result qualified a feature.
- **Continuously ingest changes and automatically correct historical results:** reduces manual intervention, but adds change tracking and reconciliation complexity.
- **Require individual corrections to saved inputs:** offers direct control, but adds manual work compared with rereading the source for the affected interval.
- **Automatically update existing Linear ticket evidence after corrections:** reduces manual ticket maintenance, but adds synchronization logic. CLI reports and human ticket updates were selected instead.
- **Block analysis on invalid rows:** avoids publishing results from an incomplete dataset, but lets individual data problems delay analysis. Exclusion with diagnostics was selected instead.

## Consequences

- Saved evidence explains earlier demand calculations despite later source changes. Preserving it adds local storage and retention responsibilities.
- Historical corrections require a person to request a rerun and maintain related ticket evidence.
- Corrected results reflect the source as read during the rerun, rather than reconstructing the original source state. New classifications can also differ from the saved original classifications.
- Excluded invalid rows can change both feature customer counts and the denominator, so results must identify that they use the valid subset.
- Copies remain subject to the retention policy. Omitting a deleted source message from a corrected result does not establish deletion of previously saved copies; point 9 still owns retention and deletion policy.

## Remaining specifications

Exact SQL definitions, database-specific types and validation, namespace configuration, consistent input capture and partial-capture/restart handling, storage of corrected results, and CLI syntax/report formatting remain open. Contract versioning/compatibility and retention/deletion policy have not been selected. Additional provenance tracking is not a committed future feature. This ADR records accepted policy, not implementation completion.

## References

- [Product design: uniform feedback schema](../01-grill-session/README.md#3-uniform-feedback-schema)
- [Product design: weekly analysis and corrections](../01-grill-session/README.md#4-weekly-analysis-and-demand-threshold)
- [Technical decision point 5: feedback contract, reproducibility, and corrections](../02-technical-decisions/README.md#5-feedback-contract-reproducibility-and-corrections)
- [Technical decision point 9: retention and copied artifacts](../02-technical-decisions/README.md#9-retention-and-copied-artifacts)
- [ADR-0003: SQLite application state](0003-use-sqlite-for-single-host-mvp-state.md)
- [ADR-0004: scheduled execution and recovery](0004-distinguish-scheduled-recovery-retries-and-manual-execution.md)
