import { expect, test } from "vite-plus/test";
import {
  compareVersions,
  declaredOrigins,
  latestVersion,
  marketErrorCode,
} from "./source-market-utils";

test("compareVersions orders semantic versions", () => {
  expect(compareVersions("1.0.0", "1.0.0")).toBe(0);
  expect(compareVersions("1.0.0", "1.0.1")).toBe(-1);
  expect(compareVersions("1.10.0", "1.9.9")).toBe(1);
  expect(compareVersions("2.0.0", "10.0.0")).toBe(-1);
  expect(compareVersions("1.0.0-alpha", "1.0.0")).toBe(-1);
  expect(compareVersions("1.0.0-alpha", "1.0.0-alpha.1")).toBe(-1);
  expect(compareVersions("1.0.0-alpha.1", "1.0.0-alpha.beta")).toBe(-1);
  expect(compareVersions("1.0.0-beta.2", "1.0.0-beta.11")).toBe(-1);
});

test("latestVersion picks the highest listed version", () => {
  const versions = [
    {
      version: "1.0.0",
      size: 1,
      resource_origins: [],
      media_origins: [],
      capabilities: [],
    },
    {
      version: "1.10.0",
      size: 2,
      resource_origins: [],
      media_origins: [],
      capabilities: [],
    },
    {
      version: "1.9.0",
      size: 3,
      resource_origins: [],
      media_origins: [],
      capabilities: [],
    },
  ];
  expect(latestVersion(versions)?.version).toBe("1.10.0");
  expect(latestVersion([])).toBeNull();
});

test("declaredOrigins combines resource and media origins without duplicates", () => {
  expect(
    declaredOrigins({
      version: "1.0.0",
      size: 1,
      resource_origins: ["https://a.example.com", "https://m.example.com"],
      media_origins: ["https://m.example.com", "https://cdn.example.com"],
      capabilities: [],
    }),
  ).toEqual([
    "https://a.example.com",
    "https://m.example.com",
    "https://cdn.example.com",
  ]);
});

test("marketErrorCode reads mock and wire error shapes", () => {
  expect(
    marketErrorCode({
      code: "consent_required",
      message: "x",
      retryable: false,
    }),
  ).toBe("consent_required");
  expect(
    marketErrorCode({
      code: "streaming_source_unavailable",
      message: "source_id_taken: Another package holds this source id.",
      retryable: false,
    }),
  ).toBe("source_id_taken");
  expect(
    marketErrorCode({
      code: "streaming_source_unavailable",
      message: "invalid_registry: registry id conflict",
      retryable: false,
    }),
  ).toBe("invalid_registry");
  expect(marketErrorCode(new Error("boom"))).toBeNull();
  expect(
    marketErrorCode({ code: "network", message: "network: down" }),
  ).toBeNull();
  expect(marketErrorCode("nope")).toBeNull();
});
