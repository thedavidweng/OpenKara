import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";

const schema = JSON.parse(
  readFileSync("./schemas/release-evidence.schema.json", "utf8"),
) as Record<string, unknown>;

function requiredFields(objectSchema: unknown): string[] {
  return (objectSchema as Record<string, unknown>).required as string[];
}

function definition(name: string): Record<string, unknown> {
  return (schema.definitions as Record<string, unknown>)[name] as Record<
    string,
    unknown
  >;
}

describe("release evidence schema", () => {
  test("requires the canonical release subject and evidence fields", () => {
    expect(requiredFields(schema)).toEqual(
      expect.arrayContaining([
        "schema_version",
        "subject",
        "status",
        "fragments",
        "artifacts",
      ]),
    );

    expect(requiredFields(definition("EvidenceSubject"))).toEqual(
      expect.arrayContaining(["repository", "commit_sha", "tag", "version"]),
    );
  });

  test("requires artifact identity and assertion status", () => {
    const artifact = (schema.definitions as Record<string, unknown>)
      .ArtifactEvidence as Record<string, unknown>;
    expect(requiredFields(artifact)).toEqual(
      expect.arrayContaining([
        "logical_name",
        "target",
        "file_name",
        "byte_size",
        "sha256",
      ]),
    );

    const assertion = (schema.definitions as Record<string, unknown>)
      .AssertionEvidence as Record<string, unknown>;
    expect(requiredFields(assertion)).toEqual(
      expect.arrayContaining(["id", "status"]),
    );
  });

  test("uses the versioned passed/failed status vocabulary", () => {
    const status = (schema.definitions as Record<string, unknown>)
      .EvidenceStatus as Record<string, unknown>;
    expect(status.enum).toEqual(["passed", "failed"]);
  });
});
