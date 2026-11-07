import type { SourceSnapshot } from "@/types/ipc";

/** A ready, installed Streaming Source snapshot for tests. */
export const source = (
  id: string,
  capabilities: string[],
  options: { enabled?: boolean; requiresSearchAuth?: boolean } = {},
): SourceSnapshot => ({
  source_id: id,
  registry_id: null,
  package_id: null,
  owner_binding: `test/${id}`,
  name: id,
  description: "Test source",
  license: "MIT",
  installed_version: "1.0.0",
  available_version: "1.0.0",
  staged_version: null,
  pinned_version: null,
  enabled: options.enabled ?? true,
  loaded: true,
  running: false,
  state: "ready",
  progress: null,
  last_error: null,
  active_work: [],
  manifest: {
    manifest_version: 1,
    source_id: id,
    package_id: id,
    publisher: "test",
    version: "1.0.0",
    minimum_host_version: "0.1.0",
    protocol: { majors: [2], minor: 0 },
    runtime: { kind: "openkara-js", major: 1 },
    entry: "index.mjs",
    name: id,
    description: "Test source",
    license: "MIT",
    capabilities,
    authentication: options.requiresSearchAuth
      ? { flow: "plugin", presentations: ["qr"] }
      : { flow: "none" },
    requires_authentication: options.requiresSearchAuth ? ["search"] : [],
    resource_origins: [],
    media_origins: [],
  },
});
