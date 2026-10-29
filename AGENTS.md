# OpenKara

Cross-platform Tauri 2 karaoke app with React/TypeScript UI and local AI stem separation.

- JavaScript package manager: **pnpm**; standard frontend build: `pnpm build`.
- Implementation code is authoritative for current behavior; use docs for constraints, rationale and contracts.
- Before changing product surfaces, read [product standards](docs/references/product-standards.md) and the relevant linked profiles; include required evidence in the PR.
- Git hooks use hk via mise; `mise install` installs them. The pre-push gate is mandatory. Never push with `--no-verify` or set `HK=0`.
- Do not leave completed plans in the repo.
- Read task-specific guidance:
  - [Engineering and docs](docs/agents/engineering.md).
  - [TypeScript and IPC changes](docs/agents/frontend-contracts.md).
  - [Model/runtime and platform constraints](docs/agents/platform.md).
  - [Verification](.agents/skills/verify/SKILL.md), [delivery](.agents/skills/delivery/SKILL.md), [CI notes](.agents/skills/ci-notes/SKILL.md).
  - [Domain glossary](CONTEXT.md), [domain docs](docs/agents/domain.md), [ADR index](docs/adr/), [issue tracker](docs/agents/issue-tracker.md).
