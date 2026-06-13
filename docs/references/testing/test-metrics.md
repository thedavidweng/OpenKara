# Test Metrics and Coverage Reporting

## Metrics Captured

### Coverage (Vitest + V8)

Every CI run on `frontend-test` produces a coverage report with four metrics:

| Metric         | Description                                   |
| -------------- | --------------------------------------------- |
| **Branches**   | Percentage of conditional branches executed   |
| **Functions**  | Percentage of functions invoked at least once |
| **Lines**      | Percentage of executable source lines covered |
| **Statements** | Percentage of individual statements executed  |

The minimum threshold for each metric is **65%** (lines/statements) and **60%** (functions/branches). If coverage drops below these thresholds the CI job fails. The lower functions/branches threshold reflects React UI component event handlers that require `@testing-library/react` or Playwright UI smoke coverage to exercise — not practical for unit tests alone.

### Test Execution Summary

The CI job writes a step summary to the GitHub Actions UI containing:

- A table of coverage percentages per metric
- Total test count, pass/fail counts, and execution time (reported by Vitest in its default output)

## Reading Coverage Reports

### CI (GitHub Actions)

1. Open a pull request or push to `main`.
2. Navigate to the **Actions** tab and select the CI workflow run.
3. Open the **Frontend tests** job.
4. The **Test metrics summary** step renders a Markdown table directly in the step summary panel.
5. On pull requests, the `vitest-coverage-report-action` posts an inline comment with a detailed per-file breakdown.

### Downloading Raw Reports

Coverage artifacts (`frontend-coverage`) are uploaded for every run with a 14-day retention period. Each artifact contains:

| File / Directory                | Format       | Purpose                                                        |
| ------------------------------- | ------------ | -------------------------------------------------------------- |
| `coverage-summary.json`         | JSON summary | Machine-readable totals for CI actions                         |
| `lcov.info`                     | LCOV         | Compatible with Codecov, SonarQube, etc.                       |
| `index.html` + supporting files | HTML         | Open `index.html` in a browser for a navigable per-file report |

### Locally

```bash
pnpm test:coverage
open coverage/index.html
```

## Test Pyramid Distribution

Current spec-file counts:

| Layer                | Location             | Count   |
| -------------------- | -------------------- | ------- |
| Unit tests           | `src/**/*.test.*`    | 109     |
| Integration          | `tests/*.test.*`     | 5       |
| Playwright UI smoke  | `tests/e2e/*.spec.*` | 8       |
| **Total spec files** |                      | **122** |

The project is heavily weighted toward unit tests, which aligns with the fast-feedback goal of the frontend test suite. Playwright UI smoke tests cover critical browser-rendered flows (library, playback, playlists, lyrics, queue/rotation, song import, and smoke) against mocked Tauri IPC. Release-only separation smoke covers the real runtime/model/separation path.

## Running Tests Locally

```bash
# Run all unit and integration tests (no coverage)
pnpm test

# Run tests with coverage report
pnpm test:coverage

# Run tests in watch mode during development
pnpm test -- --watch

# Run Playwright UI smoke tests (Vite dev server with mocked Tauri IPC)
pnpm test:ui-smoke

# Run Playwright UI smoke tests with the Playwright UI
pnpm test:ui-smoke:ui
```
