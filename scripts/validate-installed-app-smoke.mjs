import { readFileSync } from "node:fs";

function readReport(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function normalizedPath(path) {
  return path.replaceAll("\\", "/").toLowerCase();
}

function assert(condition, message, failures) {
  if (!condition) failures.push(message);
}

function hasEvent(events, event) {
  return events.some((candidate) => candidate.event === event);
}

function validateManagedPaths(report, label, failures) {
  const appData = normalizedPath(report.app_data_dir);
  assert(
    normalizedPath(report.model_path).startsWith(appData),
    `${label}: model path was outside app data (${report.model_path})`,
    failures,
  );
  assert(
    normalizedPath(report.model.model_path).startsWith(appData),
    `${label}: model status path was outside app data (${report.model.model_path})`,
    failures,
  );
  assert(
    normalizedPath(report.runtime.runtime_path).startsWith(appData),
    `${label}: runtime path was outside app data (${report.runtime.runtime_path})`,
    failures,
  );
}

export function validateInstalledAppSmokeReports(prepare, restart) {
  const failures = [];

  assert(prepare.phase === "prepare", "prepare: unexpected phase", failures);
  assert(restart.phase === "restart", "restart: unexpected phase", failures);
  assert(
    prepare.runtime.state === "ready",
    "prepare: runtime was not ready",
    failures,
  );
  assert(
    prepare.model.state === "ready",
    "prepare: model was not ready",
    failures,
  );
  assert(
    restart.runtime.state === "ready",
    "restart: runtime was not ready",
    failures,
  );
  assert(
    restart.model.state === "ready",
    "restart: model was not ready",
    failures,
  );
  validateManagedPaths(prepare, "prepare", failures);
  validateManagedPaths(restart, "restart", failures);

  assert(
    hasEvent(prepare.runtime_events, "runtime-bootstrap-progress"),
    "prepare: runtime did not perform a first-install download",
    failures,
  );
  assert(
    hasEvent(prepare.model_events, "model-bootstrap-progress"),
    "prepare: model did not perform a first-install download",
    failures,
  );
  assert(
    !hasEvent(restart.runtime_events, "runtime-bootstrap-progress"),
    "restart: runtime unexpectedly downloaded again",
    failures,
  );
  assert(
    !hasEvent(restart.model_events, "model-bootstrap-progress"),
    "restart: model unexpectedly downloaded again",
    failures,
  );

  const smoke = restart.local_audio_smoke;
  assert(
    smoke != null,
    "restart: local audio smoke report was missing",
    failures,
  );
  if (smoke) {
    const summary = smoke.summary;
    assert(
      smoke.model.status === "passed",
      "restart: smoke model was not verified",
      failures,
    );
    assert(
      summary.discovered_files === 1,
      `restart: expected 1 input, got ${summary.discovered_files}`,
      failures,
    );
    assert(
      summary.imported === 1,
      `restart: expected 1 import, got ${summary.imported}`,
      failures,
    );
    assert(
      summary.playback_failed === 0,
      `restart: playback failures ${summary.playback_failed}`,
      failures,
    );
    assert(
      summary.separation_passed >= 1,
      "restart: no separation succeeded",
      failures,
    );
    assert(
      summary.separation_failed === 0,
      `restart: separation failures ${summary.separation_failed}`,
      failures,
    );
    assert(
      summary.separation_skipped === 0,
      `restart: skipped separations ${summary.separation_skipped}`,
      failures,
    );
    assert(
      smoke.songs.some(
        (song) =>
          song.separation_status === "passed" &&
          song.vocals_path !== null &&
          song.accompaniment_path !== null,
      ),
      "restart: no song produced both vocals and accompaniment artifacts",
      failures,
    );
  }

  return failures;
}

function main() {
  const [preparePath, restartPath] = process.argv.slice(2);
  if (!preparePath || !restartPath) {
    throw new Error(
      "usage: node scripts/validate-installed-app-smoke.mjs <prepare-report> <restart-report>",
    );
  }

  const failures = validateInstalledAppSmokeReports(
    readReport(preparePath),
    readReport(restartPath),
  );
  if (failures.length > 0) {
    throw new Error(
      `Installed app release smoke failed:\n${failures.map((failure) => `- ${failure}`).join("\n")}`,
    );
  }
  console.log("Installed app release smoke passed.");
}

if (import.meta.url === new URL(process.argv[1], "file:").href) {
  main();
}
