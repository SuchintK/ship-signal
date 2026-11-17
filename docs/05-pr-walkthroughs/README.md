# PR walkthroughs

Standalone walkthroughs in this directory explain the code as it existed at the source commit identified in each file. Later changes to the PR can make a walkthrough stale; regenerate it when needed.

| PR | Walkthrough |
| --- | --- |
| 1 | [PostgreSQL analysis and human-reviewed need identities](pr-1.html) |
| 2 | [Parallel Rust and PostgreSQL CI checks](pr-2.html) |

When Codex creates a PR, it should run `.codex/hooks/pr-create` in place of `gh pr create`, with the same arguments. The hook asks Codex to complete the `pr-code-walkthrough` skill in the same session. Review and trust the repository hooks in Codex before use; the feature requires GitHub CLI authentication and permission to push to the PR branch. The automatic path only detects this wrapper, and the agent can still encounter permission prompts or stop on safety checks. For PRs created another way, ask Codex to run the skill for the PR explicitly.

## Maintaining walkthroughs

Run the repository's dependency-free Python 3.9+ tool from inside the checkout. It reads committed Git blobs, so the source ref must include the code you want to explain. Uncommitted application edits are outside that snapshot.

Validate every walkthrough, or a particular file:

```sh
python3 scripts/walkthrough.py check
python3 scripts/walkthrough.py check docs/05-pr-walkthroughs/pr-2.html
```

The check verifies source/base commits and ancestry, exact numbered snippets and captions, balanced HTML, unique IDs, heading navigation, local paths/anchors, and offline-resource constraints. Ordinary HTTP/HTTPS reference links are allowed. Each historical walkthrough is checked against its own recorded source, rather than today's code. The tool reads the existing HTML and writes nothing during validation. Git must contain the recorded commits; CI fetches full history. If a commit is unavailable locally, fetch that recorded SHA from the repository before checking again.

### Create a source snippet

Print an escaped HTML figure to insert into a human-written walkthrough:

```sh
python3 scripts/walkthrough.py snippet --source HEAD \
  --path .github/workflows/ci.yml --start 1 --end 18
```

The output includes `data-source`, `data-start`, and `data-end`, a caption, and numbered code. Include one `Analyzed source SHA: <full SHA>` and one `Base SHA: <full SHA>` in the document. Give section headings or their containing sections IDs, and link to them from a `<nav>` element.

For stable source markers, use `--start-anchor` and `--end-anchor` instead of line numbers. Each is a literal, case-sensitive substring that must match exactly one line in the source file. The selected lines are inclusive. The figure retains these values as `data-start-anchor` and `data-end-anchor`, so later refreshes can locate them again.

### Preview and apply a refresh

For the current PR's walkthrough, supply the source and base commits verified by the PR walkthrough skill. Replace `N` and the SHA placeholders below with those values:

```sh
python3 scripts/walkthrough.py refresh docs/05-pr-walkthroughs/pr-N.html \
  --source SOURCE_SHA --base BASE_SHA
```

This prints a unified diff. Add `--write` to apply it to that HTML file. Refresh updates source ranges, captions, escaped numbered code, and source/base references. Surrounding explanations remain intact. It validates the original and the complete proposed result before writing.

An unchanged snippet can move when it has one exact match. Changes inside a snippet can be included when its boundary lines remain identifiable. Changed or ambiguous boundaries require selecting a new snippet or adding unique anchors; the command exits unsuccessfully and leaves the file untouched. File renames require updating the source selection deliberately.

Review behavior, explanations, examples, test claims, and sensitive content after refreshing. A matching excerpt does not prove the explanation is correct or that tests were run. The tool prepares a reviewable diff; the existing skill handles PR identity checks and publication.

### Tool verification

```sh
python3 -m unittest discover -s scripts/tests -v
```

The CLI tests create disposable Git repositories and exercise historical source validation, previews, shifts, changed interiors, ambiguous boundaries, source anchors, failed-refresh preservation, escaped source text, and invalid HTML/links. The required **Rust checks** CI job runs these tests and validates all walkthroughs before running Rust checks.
