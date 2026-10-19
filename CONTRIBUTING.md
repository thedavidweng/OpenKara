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

## License and CLA

The project is distributed under [AGPL-3.0-only](LICENSE). Before an external
contribution is merged, its contributor must sign [CLA version 1.0](https://github.com/thedavidweng/OpenKara/blob/cc7a3e1b7e24d9bbe10c2fc9f6b39ff84328d2f1/CLA.md).
The bot links the agreement and asks you to post this comment in the PR:

> I have read the CLA Document and I hereby sign the CLA

Use your own GitHub account. No external login, OAuth authorization, or
maintainer approval comment is required. A recorded signature is reused for
future contributions to this project under the same agreement version.
Comment `recheck` to refresh a check after correcting contributor identity.

Contributors retain copyright. The CLA grants David Weng rights to distribute
accepted contributions under other open-source, commercial, and proprietary
terms, including closed-source paid or mobile editions. Users of an AGPL
release do not need to sign a CLA.

See [LICENSING.md](LICENSING.md) for historical grants and third-party licenses.

### Maintainer setup

Publish `cla-signatures` before publishing this workflow. This separate,
unprotected branch contains the versioned agreement and the signature JSON;
records start empty. Require the **`CLA` commit status** from GitHub Actions
(app ID 15368) in the default branch's protection after the workflow is live.
Do not require `CLA Assistant`: comment-triggered runs belong to the default
branch; the `CLA` status explicitly targets the checked PR commit.

Each changed agreement version needs a new pinned document URL and a new
signature-file path. Do not treat existing version 1 signatures as consent to
a changed agreement. Historical contributions are not automatically signed.

The workflow never checks out or executes PR code. The upstream action is
pinned to Vapourfly's version 2.6.1, whose repository is now archived. It checks
at most 100 commits, reads the first page of PR comments, and does not parse
coauthor trailers. An external PR opener must also be a GitHub-linked commit
author; mismatched identity fails the check. Split larger PRs, sign before the
thread grows long, and
verify any additional coauthors' acceptance during the normal rights review.
A bot exemption is not evidence of ownership; third-party and employer rights
still need to be respected.
