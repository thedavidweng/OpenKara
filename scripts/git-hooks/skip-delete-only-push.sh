#!/bin/sh
# Run a pre-push gate command, unless the push only deletes remote refs.
#
# git feeds pre-push one "<local ref> <local sha> <remote ref> <remote sha>"
# line per ref on stdin, and a deletion carries the zero sha as its local sha.
# A deletion-only push transfers no commits, so the repo-wide gates have
# nothing to verify — running minutes of tests to delete a stale branch only
# teaches people to reach for --no-verify, which is the habit these gates
# exist to prevent. A push that deletes AND updates refs still runs the gates.
#
# lefthook must pass `use_stdin: true` for the ref lines to arrive here. Empty
# stdin therefore runs the gates: if that wiring is ever lost, the failure
# mode must be "gates run unnecessarily", never "gates silently skipped".
#
# Usage (from lefthook.yml): sh scripts/git-hooks/skip-delete-only-push.sh '<command>'

ZERO_SHA=0000000000000000000000000000000000000000

refs=0
deletions=0
while read -r _local_ref local_sha _remote_ref _remote_sha; do
  refs=$((refs + 1))
  if [ "$local_sha" = "$ZERO_SHA" ]; then
    deletions=$((deletions + 1))
  fi
done

if [ "$refs" -gt 0 ] && [ "$refs" -eq "$deletions" ]; then
  echo "skipped: this push only deletes remote refs, nothing to verify" >&2
  exit 0
fi

exec sh -c "$1"
