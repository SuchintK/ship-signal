#!/usr/bin/env python3
"""Signal a single continuation after the repository's PR creation command."""
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tempfile


def main():
    event = json.load(sys.stdin)
    session = event.get('session_id')
    turn = event.get('turn_id')
    if not isinstance(session, str) or not isinstance(turn, str):
        return
    key = hashlib.sha256((session + '\0' + turn).encode()).hexdigest()
    state_dir = Path(tempfile.gettempdir()) / ('codex-pr-walkthrough-' + str(os.getuid()))
    state_dir.mkdir(mode=0o700, exist_ok=True)
    state = state_dir / key
    if event.get('hook_event_name') == 'PostToolUse':
        command = event.get('tool_input', {}).get('command', '')
        response = event.get('tool_response', '')
        response = response if isinstance(response, str) else json.dumps(response)
        # Match only an invocation of our wrapper, and only its successful marker.
        if not re.search(r'(?:^|[\s;&])(?:\./)?\.codex/hooks/pr-create(?:\s|$)', command):
            return
        match = re.search(r'CODEX_PR_WALKTHROUGH_CREATED=(https://[^\s"\\]+/[^\s/]+/[^\s/]+/pull/[0-9]+)', response)
        if not match:
            return
        state.write_text(match.group(1), encoding='utf-8')
        return {'hookSpecificOutput': {'hookEventName': 'PostToolUse', 'additionalContext': 'A PR was created. Use the repo pr-code-walkthrough skill before finishing this turn.'}}
    elif event.get('hook_event_name') == 'Stop' and not event.get('stop_hook_active'):
        try:
            url = state.read_text(encoding='utf-8')
            state.unlink()
        except FileNotFoundError:
            return
        return {'decision': 'block', 'reason': 'For the PR created in this turn (' + url + '), follow the repository pr-code-walkthrough skill. Verify the PR and branch, and report any blocker. Make only one attempt.'}


if __name__ == '__main__':
    try:
        result = main()
    except (OSError, ValueError, TypeError, AttributeError):
        result = None
    print(json.dumps(result or {}))
