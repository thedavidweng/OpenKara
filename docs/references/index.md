# References

Reference docs describe the current project reality: architecture, contracts,
product behavior, generated facts, release rules, and testing guidance. They are
not backlog items and they are not release notes.

## Sections

| Section                                                      | Purpose                                                                            |
| ------------------------------------------------------------ | ---------------------------------------------------------------------------------- |
| [`architecture/`](./architecture/)                           | Architecture, roadmap, project structure, release rules, and performance baselines |
| [`contracts/`](./contracts/)                                 | Stable IPC and backend-facing contracts                                            |
| [`product/`](./product/)                                     | User-visible behavior specs                                                        |
| [`generated/`](./generated/)                                 | Generated reference docs checked into the repo                                     |
| [`testing/`](./testing/)                                     | Smoke-test and coverage reporting guidance                                         |
| [`agents/`](./agents/)                                       | Agent-facing labels and workflow notes                                             |
| [`dropbox-review.md`](./dropbox-review.md)                   | Dropbox review scope evidence                                                      |
| [`shell-frontend-boundary.md`](./shell-frontend-boundary.md) | Desktop shell / frontend boundary                                                  |

## Rules

- Update a reference in the same change that modifies the behavior it documents.
- Do not put completion history here; use [`../../CHANGELOG.md`](../../CHANGELOG.md).
- Do not put future backlog lists here; use the
  [GitHub Project](https://github.com/users/thedavidweng/projects/2/views/1).
