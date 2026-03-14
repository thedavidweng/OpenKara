# Homebrew Cask Packaging

This directory holds the backend-owned scaffolding for shipping OpenKara through
Homebrew.

Why cask instead of formula:

- OpenKara ships as a macOS desktop application bundle.
- Homebrew Formula is a poor fit for GUI app installation.
- Homebrew Cask is the supported path for distributing signed `.dmg` assets.

Files:

- `openkara.rb.template`: template for the tap repository cask file

Typical release flow:

1. Build and publish the macOS release assets on GitHub Releases.
2. Compute the SHA-256 for the Apple Silicon and Intel `.dmg` files.
3. Run `scripts/render-homebrew-cask.sh` with the release version, URLs, and
   checksums.
4. Copy the rendered `openkara.rb` into the Homebrew tap repository.
5. Run `brew audit --cask --strict ./openkara.rb` inside the tap repo.

The tap repository is still external to this workspace. This directory exists so
future maintainers do not have to reconstruct the cask format from memory.
