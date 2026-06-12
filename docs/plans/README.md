# Plans

This folder holds executable plans only.

An executable plan is a scoped work slice that someone can pick up and complete
without rebuilding the product backlog from scratch. It must include:

- goal and non-goals
- acceptance criteria
- files or modules likely to change
- verification commands
- clear completion / archive condition

## What Does Not Belong Here

- Completed work: summarize it in [`../../CHANGELOG.md`](../../CHANGELOG.md) and
  move the detailed plan to [`../archive/`](../archive/).
- Future ideas without a ready implementation shape: track them in the
  [GitHub Project](https://github.com/users/thedavidweng/projects/2/views/1).
- Current system behavior, contracts, release rules, or testing notes: put them
  under [`../references/`](../references/).
- Generated summaries: put them under [`../references/generated/`](../references/generated/)
  and keep the generator documented in [`../../scripts/README.md`](../../scripts/README.md).

## Current State

There is no active repo-local execution plan. Use the GitHub Project to choose
the next work item, then create a dated plan here only when the work is ready to
execute.
