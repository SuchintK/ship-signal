# PR walkthroughs

Standalone walkthroughs in this directory explain the code as it existed at the source commit identified in each file. Later changes to the PR can make a walkthrough stale; regenerate it when needed.

When Codex creates a PR, it should run `.codex/hooks/pr-create` in place of `gh pr create`, with the same arguments. The hook asks Codex to complete the `pr-code-walkthrough` skill in the same session. Review and trust the repository hooks in Codex before use; the feature requires GitHub CLI authentication and permission to push to the PR branch. The automatic path only detects this wrapper, and the agent can still encounter permission prompts or stop on safety checks. For PRs created another way, ask Codex to run the skill for the PR explicitly.
