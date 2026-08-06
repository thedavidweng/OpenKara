# Issue tracker: GitHub

Issues and PRDs for this repo live as GitHub issues. Use the `gh` CLI for all
operations. Remote: `thedavidweng/OpenKara`.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`. Use a
  heredoc for multi-line bodies.
- **Read an issue**: `gh issue view <number> --comments`
- **List issues**: `gh issue list --state open --json number,title,body,labels`
- **Comment**: `gh issue comment <number> --body "..."`
- **Labels**: `gh issue edit <number> --add-label "..."` /
  `--remove-label "..."`
- **Close**: `gh issue close <number> --comment "..."`

Infer the repo from `git remote -v`. `gh` does this inside a clone.

## Pull requests as a triage surface

**PRs as a request surface: no.**

## When a skill says "publish to the issue tracker"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --comments`.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a single issue with **child** issues as
tickets.

- **Map**: issue labelled `wayfinder:map`.
- **Child ticket**: GitHub sub-issue when available; otherwise a task-list
  entry on the map and `Part of #<map>` in the child body. Labels:
  `wayfinder:<type>` (`research` / `prototype` / `grilling` / `task`).
- **Blocking**: native issue dependencies when available; otherwise
  `Blocked by: #<n>` in the child body.
- **Claim**: `gh issue edit <n> --add-assignee @me`
- **Resolve**: comment, close, then append a pointer on the map.
