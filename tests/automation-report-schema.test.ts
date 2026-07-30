import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";

const schema = JSON.parse(
  readFileSync("./schemas/automation-report.schema.json", "utf8"),
) as Record<string, unknown>;

function requiredFields(objectSchema: unknown): string[] {
  return (objectSchema as Record<string, unknown>).required as string[];
}

describe("automation report schema", () => {
  test("requires the canonical top-level fields", () => {
    expect(requiredFields(schema)).toEqual(
      expect.arrayContaining([
        "scenario",
        "status",
        "started_at",
        "finished_at",
        "duration_ms",
        "application",
        "environment",
        "steps",
        "assertions",
        "artifacts",
        "runtime",
        "model",
        "database",
        "accessibility",
        "audio",
        "errors",
      ]),
    );
  });

  test("requires application and environment identity fields", () => {
    const application = (schema.properties as Record<string, unknown>)
      .application as Record<string, unknown>;
    const environment = (schema.properties as Record<string, unknown>)
      .environment as Record<string, unknown>;

    expect(requiredFields(application)).toEqual(
      expect.arrayContaining(["name", "version", "commit_sha"]),
    );
    expect(requiredFields(environment)).toEqual(
      expect.arrayContaining([
        "os_version",
        "webview2_version",
        "selected_execution_provider",
      ]),
    );
  });

  test("requires step and assertion fields", () => {
    const steps = (schema.properties as Record<string, unknown>)
      .steps as Record<string, unknown>;
    const stepItem = steps.items as Record<string, unknown>;

    expect(requiredFields(stepItem)).toEqual(
      expect.arrayContaining([
        "id",
        "name",
        "status",
        "started_at",
        "finished_at",
        "duration_ms",
      ]),
    );

    const assertions = (schema.properties as Record<string, unknown>)
      .assertions as Record<string, unknown>;
    const assertionItem = assertions.items as Record<string, unknown>;

    expect(requiredFields(assertionItem)).toEqual(
      expect.arrayContaining([
        "id",
        "expected",
        "observed",
        "result",
        "artifact_path",
      ]),
    );
  });

  test("requires runtime and model identity fields", () => {
    const runtime = (schema.properties as Record<string, unknown>)
      .runtime as Record<string, unknown>;
    const model = (schema.properties as Record<string, unknown>)
      .model as Record<string, unknown>;

    expect(requiredFields(runtime)).toEqual(
      expect.arrayContaining([
        "archive_sha256",
        "extracted_library_sha256",
        "companion_dll_sha256s",
      ]),
    );
    expect(requiredFields(model)).toEqual(
      expect.arrayContaining([
        "archive_sha256",
        "extracted_onnx_sha256",
        "verification_manifest",
        "catalog_generation",
        "release_id",
        "artifact_id",
        "selected_variant",
      ]),
    );
  });

  test("requires database, accessibility, audio, and error fields", () => {
    const database = (schema.properties as Record<string, unknown>)
      .database as Record<string, unknown>;
    const accessibility = (schema.properties as Record<string, unknown>)
      .accessibility as Record<string, unknown>;
    const audio = (schema.properties as Record<string, unknown>)
      .audio as Record<string, unknown>;

    expect(requiredFields(database)).toEqual(
      expect.arrayContaining(["schema_version", "path"]),
    );
    expect(requiredFields(accessibility)).toEqual(
      expect.arrayContaining([
        "violations_count",
        "keyboard_trap_count",
        "ui_automation_errors_count",
      ]),
    );
    expect(requiredFields(audio)).toEqual(
      expect.arrayContaining([
        "sample_rate",
        "channel_count",
        "non_silent_samples",
      ]),
    );

    const errors = (schema.properties as Record<string, unknown>)
      .errors as Record<string, unknown>;
    const errorItem = errors.items as Record<string, unknown>;
    expect(requiredFields(errorItem)).toEqual(
      expect.arrayContaining(["message"]),
    );
  });
});
