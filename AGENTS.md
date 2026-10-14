# Repository instructions

## Documentation naming

- Organize top-level documentation topics in `docs/NN-kebab-case-topic/`, using a two-digit, zero-padded prefix, such as `docs/02-technical-decisions/`.
- Assign new topic folders the next number after the highest existing prefix. Preserve existing folder numbers when adding topics.
- Use `README.md` as each topic folder's entry point and keep the topic index in `docs/README.md` updated.
- Keep ADRs in `docs/03-adr/`. Number individual records independently with four-digit prefixes: `NNNN-kebab-case-decision.md`. Maintain their index in `docs/03-adr/README.md`.
- When moving or renaming documentation, update all affected links in the same change and verify that local paths and anchors resolve.
