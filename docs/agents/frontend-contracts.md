# TypeScript and public IPC

- Do not escape static checking via `as any`, `@ts-ignore`, or `@ts-expect-error`; correct the types.
- When changing a public IPC command, event, payload or source enum, update the matching canonical contract under `docs/references/contracts/` in the same change. See `docs/agents/engineering.md` for broader documentation rules.
- Use the existing formatting and verification skills. Standard formatting includes `pnpm format:write` and `cargo fmt`; hook failures must be corrected rather than bypassed.
- When working in `pnpm-lock.yaml`, `website/`, `src-tauri/tauri.conf.json` or workflow YAML, validate formatting and generated assets relevant to the touched surface.
