# Contributing

Thanks for your interest in contributing.

## Getting Started

```bash
git clone https://github.com/thedavidweng/OpenKara.git
cd OpenKara
mise install        # install tools pinned in mise.toml
pnpm install
./scripts/setup.sh  # download the separation model and ONNX Runtime for local dev
```

## Development

```bash
pnpm tauri dev      # dev server with hot reload
pnpm tauri build    # release bundle
```

## Checks

Git hooks run most of these for you. `pre-commit` formats the staged files
and runs knip. `pre-push` runs the format check, the lint, knip, and patch
coverage. Run the rest before you open a pull request.

```bash
node --run lint                                  # frontend lint
node --run build                                 # typecheck and build the frontend
pnpm vitest run                                  # frontend tests
node --run check:i18n                            # locale key parity

cd src-tauri
cargo clippy --all-targets -- -D warnings        # Rust lint
cargo nextest run                                # Rust tests
```

## Pull Requests

1. Fork the repository and create a feature branch.
2. Make your changes. Add tests when the change has behavior to pin.
3. Run the checks above for the areas you touched.
4. Update `docs/references/contracts/*.md` in the same change when you change a
   public IPC command, payload, or event.
5. Read the applicable profile in
   [`docs/references/product-standards.md`](docs/references/product-standards.md).
   Put its automated or manual evidence, or a documented exception, in the PR.
6. Open a pull request against `main`. The title must follow Conventional
   Commits, because CI checks it.

## Commit Messages

This project follows [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` new feature
- `fix:` bug fix
- `docs:` documentation only
- `chore:` maintenance task
- `refactor:` a code change. It does not fix a bug or add a feature.
- `test:` add or update tests

## Releasing

Maintainers: see [docs/RELEASING.md](docs/RELEASING.md) for the release-please
process (merge the release PR; the Release workflow builds, smokes, publishes,
and submits distribution PRs).

## License

If you contribute, you accept the project license for your contributions.
