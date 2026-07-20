# Contributing

Thanks for your interest in contributing.

## Getting Started

```bash
git clone https://github.com/thedavidweng/OpenKara.git
cd OpenKara
mise install  # install tools pinned in mise.toml
pnpm install
```

## Development

```bash
# Start dev server (hot-reload)
pnpm tauri dev

# Build release binary
pnpm tauri build

# Run Rust linter
cargo clippy

# Format code
cargo fmt

# Run tests
cargo test
```

## Pull Requests

1. Fork the repository and create a feature branch.
2. Make your changes with tests if applicable.
3. Run `cargo clippy` and `cargo fmt` before you commit.
4. Open a pull request against `main`.

## Commit Messages

This project follows [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` new feature
- `fix:` bug fix
- `docs:` documentation only
- `chore:` maintenance task
- `refactor:` a code change. It does not fix a bug or add a feature.
- `test:` add or update tests

## License

If you contribute, you accept the project license for your contributions.
