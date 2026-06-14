# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### ♻️ Refactoring

- Modularize root navigation, add web-specific layout, and introduce data source mode toggle with mock scripts.
- Remediate shell audit findings with explicit focus states, unified map action primitives, and measured welcome card positioning
- Extract mappers and remove auth wrappers to reduce repository entropy
- Extract seams, split stores, and decompose god screen

### ✨ New Features

- Initialize project with README documentation and gitignore configuration
- Initialize RideWatch project with Expo router, Supabase integration, and core architecture documentation

### 📝 Documentation

- Add AGENTS.md for session management and update README with MapLibre migration and project structure changes

### 🔧 Chores

- Checkpoint pnpm migration and remove cached artifacts
- Update dependencies to latest
- Add npm audit auto-block to CI

### 🧪 Tests

- Add 119 meaningful tests for mappers, format, map, store, and edge functions
- Fix stage-4-copy to read from correct panel files
