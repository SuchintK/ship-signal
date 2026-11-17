# Repository instructions

## Documentation naming

- Organize top-level documentation topics in `docs/NN-kebab-case-topic/`, using a two-digit, zero-padded prefix, such as `docs/02-technical-decisions/`.
- Assign new topic folders the next number after the highest existing prefix. Preserve existing folder numbers when adding topics.
- Use `README.md` as each topic folder's entry point and keep the topic index in `docs/README.md` updated.
- Keep ADRs in `docs/03-adr/`. Number individual records independently with four-digit prefixes: `NNNN-kebab-case-decision.md`. Maintain their index in `docs/03-adr/README.md`.
- When moving or renaming documentation, update all affected links in the same change and verify that local paths and anchors resolve.
- When changing database schemas, feedback contracts, persisted JSON, relationships, or storage behavior, update the [database design](docs/06-database-design/database-design.html) in the same change, including affected diagrams, examples, and source-version references.

## Agent skills

### Issue tracker

Track Ship Signal's own development specs and tickets in Linear through the connected Linear tools.

- **Workspace:** [Suchint Kaushik](https://linear.app/suchint)
- **Team:** SUC (Suchint Kaushik)
- **Team ID:** `053ec34c-422d-4d00-aabf-8ce027b84b7d`
- **Repository:** [SuchintK/ship-signal](https://github.com/SuchintK/ship-signal)
- **Project:** none configured; create issues in SUC without a project association.

Before creating an issue, search SUC for matching work to avoid duplicates. Include "Ship Signal" in new issue titles and link the repository to distinguish this project's work within the team. When fetching ticket context, read the issue and its discussion. When a skill says to publish to the issue tracker, create a Linear issue with its required content and labels. Preserve unrelated fields and labels when updating existing issues.

### Scope changes

When a requested or discovered change alters the scope, behavior, assumptions, or acceptance criteria of Ship Signal development work, use the [scope-impact skill](.agents/skills/scope-impact/SKILL.md) to check and report downstream impact without waiting for a separate request.

### Triage labels

These labels classify **development work to build Ship Signal**, implemented by a person or an agent. They do not configure the customer-facing workflow that Ship Signal enables. Customer-generated tickets use each customer team's configured Linear statuses, approval actions, and existing tagging conventions.

| Canonical role | Linear label | Meaning |
| --- | --- | --- |
| `needs-triage` | `needs-triage` | Maintainer evaluation required |
| `needs-info` | `needs-info` | More information required |
| `ready-for-agent` | `ready-for-agent` | Fully specified for agent implementation |
| `ready-for-human` | `ready-for-human` | Human implementation required |
| `wontfix` | `wontfix` | Will not be actioned |

Use these exact label names in SUC. Check for an existing applicable label before creating one, and preserve unrelated labels when applying or removing triage labels.

### Domain docs

This repository uses a single-context domain model.

Before exploring a topic, read the root [glossary](GLOSSARY.md), the [ADR index](docs/03-adr/README.md), and applicable ADRs. Consult the [settled product design](docs/01-grill-session/README.md) and [technical decisions](docs/02-technical-decisions/README.md) for accepted behavior and unresolved choices.

- Use glossary vocabulary in specs, tickets, code, and tests.
- Distinguish accepted decisions from proposals, unresolved choices, and deferred scope.
- Surface conflicts with accepted ADRs before proceeding and preserve accepted decision history.
- If an optional domain document is absent, proceed without creating it solely to satisfy this configuration.

Keep this skills configuration in `AGENTS.md`.

### PR walkthroughs

When creating a GitHub PR, run `.codex/hooks/pr-create` with the arguments you would pass to `gh pr create`, then follow the [PR code walkthrough skill](.agents/skills/pr-code-walkthrough/SKILL.md).
