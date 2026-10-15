# AI models and platform build constraints

- `src-tauri/models/` is a local development/test cache. It must not become the end-user model or runtime installation location; shipped versions use the application-data directory.
- The Linux CI dependency `libasound2-dev` is required by the current audio stack. Do not remove it unless the audio stack is intentionally changed and the platform builds are updated.
- Product-level standards require an ADR when introducing a new product surface or changing a standards target; start at `docs/references/product-standards.md` and load only matching profiles.
- Implementation details must be checked against current code rather than inferred from old plans.
