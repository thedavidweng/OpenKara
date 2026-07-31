import {
  appendFileSync,
  existsSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SUMMARY_PATH = process.env.GITHUB_STEP_SUMMARY;

function readReport(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function normalizedPath(path) {
  return path.replaceAll("\\", "/").toLowerCase();
}

function hasEvent(events, event) {
  return events.some((candidate) => candidate.event === event);
}

function isSha256Hex(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/i.test(value);
}

async function sha256File(path) {
  const hash = createHash("sha256");
  const file = await readFile(path);
  hash.update(file);
  return hash.digest("hex").toLowerCase();
}

function appendSummary(line) {
  if (SUMMARY_PATH) {
    appendFileSync(SUMMARY_PATH, `${line}\n`);
  }
}

function pushAssertion(
  result,
  id,
  expected,
  observed,
  pass,
  artifactPath = "",
) {
  const entry = {
    id,
    expected: String(expected),
    observed: String(observed),
    result: pass ? "pass" : "fail",
    artifact_path: artifactPath,
  };
  result.assertions.push(entry);
  if (!pass) {
    result.failures.push(
      `[${id}] expected: ${entry.expected}; observed: ${entry.observed}`,
    );
  }
  appendSummary(
    `| \`${id}\` | ${entry.result} | \`${entry.expected}\` | \`${entry.observed}\` |`,
  );
  return pass;
}

function validateManagedPaths(report, label, result) {
  const appData = normalizedPath(report.app_data_dir);
  pushAssertion(
    result,
    `OKA-MANAGED-MODEL-PATH-${label.toUpperCase()}`,
    "inside app data",
    report.model_path,
    normalizedPath(report.model_path).startsWith(appData),
    report.model_path,
  );
  pushAssertion(
    result,
    `OKA-MANAGED-MODEL-STATUS-PATH-${label.toUpperCase()}`,
    "inside app data",
    report.model.model_path,
    normalizedPath(report.model.model_path).startsWith(appData),
    report.model.model_path,
  );
  pushAssertion(
    result,
    `OKA-MANAGED-RUNTIME-PATH-${label.toUpperCase()}`,
    "inside app data",
    report.runtime.runtime_path,
    normalizedPath(report.runtime.runtime_path).startsWith(appData),
    report.runtime.runtime_path,
  );
}

function parseAudioInfo(path) {
  const buffer = readFileSync(path);
  if (buffer.length < 12) {
    throw new Error(`${path}: file too short for audio header`);
  }
  const magic = buffer.toString("ascii", 0, 4);
  if (magic === "OggS") {
    return parseOggVorbisInfo(path, buffer);
  }
  return parseWavInfo(path, buffer);
}

function parseWavInfo(path, buffer) {
  if (buffer.length < 12) {
    throw new Error(`${path}: file too short for RIFF header`);
  }
  const riff = buffer.toString("ascii", 0, 4);
  const wave = buffer.toString("ascii", 8, 12);
  if (riff !== "RIFF" || wave !== "WAVE") {
    throw new Error(`${path}: not a WAV file (${riff}/${wave})`);
  }

  let offset = 12;
  let fmt = null;
  let dataOffset = null;
  let dataSize = null;
  let factSamples = null;

  while (offset < buffer.length) {
    if (offset + 8 > buffer.length) break;
    const chunkId = buffer.toString("ascii", offset, offset + 4);
    const chunkSize = buffer.readUInt32LE(offset + 4);
    const chunkStart = offset + 8;
    const nextOffset = chunkStart + chunkSize + (chunkSize % 2 ? 1 : 0);

    if (chunkId === "fmt ") {
      if (chunkSize < 16) {
        throw new Error(`${path}: fmt chunk too small`);
      }
      const formatTag = buffer.readUInt16LE(chunkStart);
      const channels = buffer.readUInt16LE(chunkStart + 2);
      const sampleRate = buffer.readUInt32LE(chunkStart + 4);
      const byteRate = buffer.readUInt32LE(chunkStart + 8);
      const blockAlign = buffer.readUInt16LE(chunkStart + 12);
      const bitsPerSample = buffer.readUInt16LE(chunkStart + 14);
      const bytesPerSample = bitsPerSample / 8;
      fmt = {
        formatTag,
        channels,
        sampleRate,
        byteRate,
        blockAlign,
        bitsPerSample,
        bytesPerSample,
        isFloat: formatTag === 3,
        isPcm: formatTag === 1,
      };
    } else if (chunkId === "data") {
      dataOffset = chunkStart;
      dataSize = chunkSize;
    } else if (chunkId === "fact" && chunkSize >= 4) {
      factSamples = buffer.readUInt32LE(chunkStart);
    }

    offset = nextOffset;
  }

  if (!fmt || dataOffset === null || dataSize === null) {
    throw new Error(`${path}: missing fmt or data chunk`);
  }

  const totalSamples = dataSize / (fmt.channels * fmt.bytesPerSample);
  const durationSeconds = Number(totalSamples) / fmt.sampleRate;

  return {
    format: "wav",
    path,
    ...fmt,
    dataOffset,
    dataSize,
    totalSamples,
    durationSeconds,
    factSamples,
  };
}

function parseOggVorbisInfo(path, buffer) {
  const OGG_MAX_GRANULE = 0xffffffffffffffffn;
  let offset = 0;
  let pageCount = 0;
  let channels = null;
  let sampleRate = null;
  let maxGranule = 0;

  while (offset < buffer.length) {
    if (buffer.length - offset < 27) break;
    if (buffer.toString("ascii", offset, offset + 4) !== "OggS") break;

    const numSegments = buffer[offset + 26];
    const headerSize = 27 + numSegments;
    if (buffer.length - offset < headerSize) break;

    let bodySize = 0;
    for (let i = 0; i < numSegments; i++) {
      bodySize += buffer[offset + 27 + i];
    }

    const dataOffset = offset + headerSize;
    const nextOffset = dataOffset + bodySize;
    if (nextOffset > buffer.length) break;

    if (pageCount === 0 && bodySize >= 30) {
      // Vorbis identification packet starts with packet type 1 + "vorbis".
      if (
        buffer[dataOffset] === 1 &&
        buffer.toString("ascii", dataOffset + 1, dataOffset + 7) === "vorbis"
      ) {
        channels = buffer[dataOffset + 11];
        sampleRate = buffer.readUInt32LE(dataOffset + 12);
      }
    }

    const granule = buffer.readBigUInt64LE(offset + 6);
    if (granule >= 0n && granule < OGG_MAX_GRANULE) {
      const granuleNumber = Number(granule);
      if (granuleNumber > maxGranule) {
        maxGranule = granuleNumber;
      }
    }

    offset = nextOffset;
    pageCount += 1;
  }

  if (sampleRate == null || channels == null) {
    throw new Error(`${path}: not a valid Ogg Vorbis file`);
  }

  const durationSeconds = maxGranule / sampleRate;

  return {
    format: "ogg",
    path,
    channels,
    sampleRate,
    totalSamples: maxGranule,
    durationSeconds,
    dataOffset: 0,
    dataSize: 0,
    bitsPerSample: 0,
    bytesPerSample: 0,
    isFloat: false,
    isPcm: false,
  };
}

function readWavSamples(info, onSample) {
  const buffer = readFileSync(info.path);
  const {
    dataOffset,
    dataSize,
    channels,
    bytesPerSample,
    bitsPerSample,
    isFloat,
  } = info;
  const end = dataOffset + dataSize;

  for (
    let byteOffset = dataOffset;
    byteOffset < end;
    byteOffset += channels * bytesPerSample
  ) {
    for (let channel = 0; channel < channels; channel++) {
      const channelOffset = byteOffset + channel * bytesPerSample;
      let floatValue = 0;

      if (isFloat) {
        if (bytesPerSample === 4) {
          floatValue = buffer.readFloatLE(channelOffset);
        } else if (bytesPerSample === 8) {
          floatValue = buffer.readDoubleLE(channelOffset);
        }
      } else if (bitsPerSample === 16) {
        floatValue = buffer.readInt16LE(channelOffset) / 32768;
      } else if (bitsPerSample === 24) {
        const b0 = buffer[channelOffset];
        const b1 = buffer[channelOffset + 1];
        const b2 = buffer[channelOffset + 2];
        let value = (b0 | (b1 << 8) | (b2 << 16)) >>> 0;
        if (value & 0x800000) {
          value -= 0x1000000;
        }
        floatValue = value / 8388608;
      } else if (bitsPerSample === 32) {
        floatValue = buffer.readInt32LE(channelOffset) / 2147483648;
      }

      onSample(floatValue, isFloat);
    }
  }
}

function validateAudioFile(path, label, result) {
  let info;
  try {
    info = parseAudioInfo(path);
  } catch (error) {
    pushAssertion(
      result,
      `OKA-AUDIO-HEADER-${label}`,
      "valid audio (WAV or Ogg Vorbis)",
      error.message,
      false,
      path,
    );
    return null;
  }

  pushAssertion(
    result,
    `OKA-AUDIO-HEADER-${label}`,
    "valid audio (WAV or Ogg Vorbis)",
    `valid ${info.format === "ogg" ? "Ogg Vorbis" : "WAV"}`,
    true,
    path,
  );

  return info;
}

function validateOutputAgainstInput(inputInfo, outputInfo, label, result) {
  pushAssertion(
    result,
    `OKA-AUDIO-SAMPLE-RATE-${label}`,
    inputInfo.sampleRate,
    outputInfo.sampleRate,
    outputInfo.sampleRate === inputInfo.sampleRate,
    outputInfo.path,
  );

  pushAssertion(
    result,
    `OKA-AUDIO-CHANNELS-${label}`,
    inputInfo.channels,
    outputInfo.channels,
    outputInfo.channels === inputInfo.channels,
    outputInfo.path,
  );

  const durationDelta = Math.abs(
    outputInfo.durationSeconds - inputInfo.durationSeconds,
  );
  const DURATION_TOLERANCE_SECONDS = 1.0;
  pushAssertion(
    result,
    `OKA-AUDIO-DURATION-${label}`,
    `<= ${DURATION_TOLERANCE_SECONDS}s delta`,
    `${durationDelta.toFixed(4)}s`,
    durationDelta <= DURATION_TOLERANCE_SECONDS,
    outputInfo.path,
  );
}

function validateOutputSamples(path, label, result) {
  let hasNonSilent = false;
  let hasInvalidFloat = false;
  const SILENCE_THRESHOLD = 1e-4;

  const info = parseAudioInfo(path);
  if (info.format !== "wav") {
    // Sample-level validation is only supported for WAV. The local-audio-smoke
    // run has already verified the actual output by decoding it.
    return;
  }

  readWavSamples(info, (value, isFloat) => {
    if (Math.abs(value) > SILENCE_THRESHOLD) {
      hasNonSilent = true;
    }
    if (isFloat && !Number.isFinite(value)) {
      hasInvalidFloat = true;
    }
  });

  pushAssertion(
    result,
    `OKA-AUDIO-NON-SILENT-${label}`,
    "contains non-silent samples",
    hasNonSilent ? "non-silent samples found" : "all samples silent",
    hasNonSilent,
    path,
  );

  pushAssertion(
    result,
    `OKA-AUDIO-NO-NAN-${label}`,
    "no NaN or infinite samples",
    hasInvalidFloat ? "NaN or infinite sample found" : "all samples finite",
    !hasInvalidFloat,
    path,
  );
}

async function validateStemsAreDifferent(
  vocalsPath,
  accompanimentPath,
  result,
) {
  const [vocalsHash, accompHash] = await Promise.all([
    sha256File(vocalsPath),
    sha256File(accompanimentPath),
  ]);
  pushAssertion(
    result,
    "OKA-AUDIO-STEMS-DIFFERENT",
    "vocals and accompaniment are not byte-identical",
    vocalsHash === accompHash ? "stems are byte-identical" : "stems differ",
    vocalsHash !== accompHash,
    `${vocalsPath}; ${accompanimentPath}`,
  );
}

function readJsonRecord(path) {
  if (!existsSync(path)) {
    return null;
  }
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return null;
  }
}

function fileRecordFor(records, filename) {
  return records.find((record) => record.path === filename) ?? null;
}

async function validateRuntimeIdentity(report, result) {
  const runtimePath = report.runtime.runtime_path;
  const recordPath = join(dirname(runtimePath), "record.json");
  const record = readJsonRecord(recordPath);

  pushAssertion(
    result,
    "OKA-284-RUNTIME-ARCHIVE-DIGEST",
    "archive digest is present and 64-hex",
    record?.archive_sha256 ?? "missing record",
    record != null && isSha256Hex(record.archive_sha256),
    recordPath,
  );

  const expectedLibrary = record
    ? fileRecordFor(record.files, basename(runtimePath))
    : null;
  const actualLibraryDigest = await sha256File(runtimePath).catch(() => null);

  pushAssertion(
    result,
    "OKA-284-RUNTIME-FILE-DIGEST",
    expectedLibrary?.sha256 ?? "unknown",
    actualLibraryDigest ?? "could not hash",
    expectedLibrary != null &&
      isSha256Hex(expectedLibrary.sha256) &&
      actualLibraryDigest === expectedLibrary.sha256.toLowerCase(),
    runtimePath,
  );

  if (record && record.files) {
    for (const file of record.files) {
      if (file.path === basename(runtimePath)) continue;
      const filePath = join(dirname(runtimePath), file.path);
      if (!existsSync(filePath)) {
        pushAssertion(
          result,
          `OKA-284-RUNTIME-COMPANION-${file.path}`,
          file.sha256,
          "file missing",
          false,
          filePath,
        );
        continue;
      }
      const actual = await sha256File(filePath).catch(() => null);
      pushAssertion(
        result,
        `OKA-284-RUNTIME-COMPANION-${file.path}`,
        file.sha256,
        actual ?? "could not hash",
        isSha256Hex(file.sha256) && actual === file.sha256.toLowerCase(),
        filePath,
      );
    }
  }
}

async function validateModelIdentity(report, result) {
  const modelPath = report.model.model_path;
  const recordPath = `${modelPath}.identity.json`;
  const record = readJsonRecord(recordPath);

  pushAssertion(
    result,
    "OKA-284-MODEL-ARCHIVE-DIGEST",
    "archive digest is present and 64-hex",
    record?.archive_sha256 ?? "missing record",
    record != null && isSha256Hex(record.archive_sha256),
    recordPath,
  );

  const expectedModel = record ? record.files[0] : null;
  const actualModelDigest = await sha256File(modelPath).catch(() => null);

  pushAssertion(
    result,
    "OKA-284-MODEL-FILE-DIGEST",
    expectedModel?.sha256 ?? "unknown",
    actualModelDigest ?? "could not hash",
    expectedModel != null &&
      isSha256Hex(expectedModel.sha256) &&
      actualModelDigest === expectedModel.sha256.toLowerCase(),
    modelPath,
  );
}

function looksLikeAbsolutePath(rawPath) {
  return (
    isAbsolute(rawPath) ||
    /^[A-Za-z]:[\\/]/.test(rawPath) ||
    /^\\\\\?\\/.test(rawPath)
  );
}

function resolveAudioPath(baseDir, rawPath) {
  if (!rawPath) return rawPath;
  if (looksLikeAbsolutePath(rawPath)) return rawPath;
  return resolve(baseDir, rawPath);
}

async function validateAudioOutputs(restart, result) {
  const smoke = restart.local_audio_smoke;
  if (!smoke) {
    return;
  }

  const songWithStems = smoke.songs.find(
    (song) =>
      song.separation_status === "passed" &&
      song.vocals_path != null &&
      song.accompaniment_path != null,
  );
  if (!songWithStems) {
    pushAssertion(
      result,
      "OKA-AUDIO-STEMS-EXIST",
      "one song with both stems",
      "no song with both stems",
      false,
      "",
    );
    return;
  }

  const inputPath = resolveAudioPath(
    smoke.input_dir,
    songWithStems.source_path,
  );
  const vocalsPath = resolveAudioPath(
    smoke.output_dir,
    songWithStems.vocals_path,
  );
  const accompPath = resolveAudioPath(
    smoke.output_dir,
    songWithStems.accompaniment_path,
  );

  if (!existsSync(inputPath)) {
    pushAssertion(
      result,
      "OKA-AUDIO-INPUT-EXISTS",
      "input file exists",
      inputPath,
      false,
      inputPath,
    );
    return;
  }

  const inputInfo = validateAudioFile(inputPath, "INPUT", result);
  if (!inputInfo) return;

  const vocalsInfo = validateAudioFile(vocalsPath, "VOCALS", result);
  const accompInfo = validateAudioFile(accompPath, "ACCOMPANIMENT", result);

  if (vocalsInfo) {
    validateOutputAgainstInput(inputInfo, vocalsInfo, "VOCALS", result);
    validateOutputSamples(vocalsPath, "VOCALS", result);
  }
  if (accompInfo) {
    validateOutputAgainstInput(inputInfo, accompInfo, "ACCOMPANIMENT", result);
    validateOutputSamples(accompPath, "ACCOMPANIMENT", result);
  }

  if (vocalsPath && accompPath) {
    try {
      await validateStemsAreDifferent(vocalsPath, accompPath, result);
    } catch (error) {
      pushAssertion(
        result,
        "OKA-AUDIO-STEMS-DIFFERENT",
        "vocals and accompaniment are not byte-identical",
        error.message,
        false,
        `${vocalsPath}; ${accompPath}`,
      );
    }
  }

  const playback = songWithStems.performance?.playback;
  if (playback) {
    const configuredSeeks = 32;
    pushAssertion(
      result,
      "OKA-SEEK-COUNT",
      configuredSeeks,
      playback.seek_samples,
      playback.seek_samples === configuredSeeks,
      "",
    );

    const SEEK_LATENCY_THRESHOLD_MS = 500;
    pushAssertion(
      result,
      "OKA-SEEK-LATENCY-MAX",
      `< ${SEEK_LATENCY_THRESHOLD_MS}ms`,
      `${playback.seek_latency_max_ms.toFixed(2)}ms`,
      playback.seek_latency_max_ms < SEEK_LATENCY_THRESHOLD_MS,
      "",
    );

    const SEEK_P95_THRESHOLD_MS = 300;
    pushAssertion(
      result,
      "OKA-SEEK-LATENCY-P95",
      `< ${SEEK_P95_THRESHOLD_MS}ms`,
      `${playback.seek_latency_p95_ms.toFixed(2)}ms`,
      playback.seek_latency_p95_ms < SEEK_P95_THRESHOLD_MS,
      "",
    );
  }
}

export async function validateInstalledAppSmokeReports(prepare, restart) {
  const result = {
    assertions: [],
    failures: [],
    measurements: {},
  };

  appendSummary("## Installed app smoke assertions");
  appendSummary("");
  appendSummary("| Assertion | Result | Expected | Observed |");
  appendSummary("| --- | --- | --- | --- |");

  pushAssertion(
    result,
    "OKA-PHASE-PREPARE",
    "prepare",
    prepare.phase,
    prepare.phase === "prepare",
    "",
  );
  pushAssertion(
    result,
    "OKA-PHASE-RESTART",
    "restart",
    restart.phase,
    restart.phase === "restart",
    "",
  );

  pushAssertion(
    result,
    "OKA-RUNTIME-READY-PREPARE",
    "ready",
    prepare.runtime.state,
    prepare.runtime.state === "ready",
    "",
  );
  pushAssertion(
    result,
    "OKA-MODEL-READY-PREPARE",
    "ready",
    prepare.model.state,
    prepare.model.state === "ready",
    "",
  );
  pushAssertion(
    result,
    "OKA-RUNTIME-READY-RESTART",
    "ready",
    restart.runtime.state,
    restart.runtime.state === "ready",
    "",
  );
  pushAssertion(
    result,
    "OKA-MODEL-READY-RESTART",
    "ready",
    restart.model.state,
    restart.model.state === "ready",
    "",
  );

  validateManagedPaths(prepare, "prepare", result);
  validateManagedPaths(restart, "restart", result);

  pushAssertion(
    result,
    "OKA-RUNTIME-FIRST-INSTALL",
    "runtime-bootstrap-progress present",
    hasEvent(prepare.runtime_events, "runtime-bootstrap-progress")
      ? "present"
      : "missing",
    hasEvent(prepare.runtime_events, "runtime-bootstrap-progress"),
    "",
  );
  pushAssertion(
    result,
    "OKA-MODEL-FIRST-INSTALL",
    "model-bootstrap-progress present",
    hasEvent(prepare.model_events, "model-bootstrap-progress")
      ? "present"
      : "missing",
    hasEvent(prepare.model_events, "model-bootstrap-progress"),
    "",
  );
  pushAssertion(
    result,
    "OKA-284-RUNTIME-COLD-RESTART",
    "no runtime-bootstrap-progress on restart",
    hasEvent(restart.runtime_events, "runtime-bootstrap-progress")
      ? "unexpected download"
      : "no re-download",
    !hasEvent(restart.runtime_events, "runtime-bootstrap-progress"),
    "",
  );
  pushAssertion(
    result,
    "OKA-284-MODEL-COLD-RESTART",
    "no model-bootstrap-progress on restart",
    hasEvent(restart.model_events, "model-bootstrap-progress")
      ? "unexpected download"
      : "no re-download",
    !hasEvent(restart.model_events, "model-bootstrap-progress"),
    "",
  );

  const smoke = restart.local_audio_smoke;
  pushAssertion(
    result,
    "OKA-LOCAL-AUDIO-SMOKE-REPORT",
    "present",
    smoke != null ? "present" : "missing",
    smoke != null,
    "",
  );

  if (smoke) {
    const summary = smoke.summary;
    pushAssertion(
      result,
      "OKA-SMOKE-MODEL-VERIFIED",
      "passed",
      smoke.model.status,
      smoke.model.status === "passed",
      "",
    );
    pushAssertion(
      result,
      "OKA-SMOKE-DISCOVERED-FILES",
      1,
      summary.discovered_files,
      summary.discovered_files === 1,
      "",
    );
    pushAssertion(
      result,
      "OKA-SMOKE-IMPORTED",
      1,
      summary.imported,
      summary.imported === 1,
      "",
    );
    pushAssertion(
      result,
      "OKA-SMOKE-PLAYBACK-FAILURES",
      0,
      summary.playback_failed,
      summary.playback_failed === 0,
      "",
    );
    pushAssertion(
      result,
      "OKA-SMOKE-SEPARATION-PASSED",
      ">= 1",
      summary.separation_passed,
      summary.separation_passed >= 1,
      "",
    );
    pushAssertion(
      result,
      "OKA-SMOKE-SEPARATION-FAILURES",
      0,
      summary.separation_failed,
      summary.separation_failed === 0,
      "",
    );
    pushAssertion(
      result,
      "OKA-SMOKE-SEPARATION-SKIPPED",
      0,
      summary.separation_skipped,
      summary.separation_skipped === 0,
      "",
    );
    pushAssertion(
      result,
      "OKA-SMOKE-STEMS-PRODUCED",
      "one song with both stems",
      smoke.songs.some(
        (song) =>
          song.separation_status === "passed" &&
          song.vocals_path != null &&
          song.accompaniment_path != null,
      )
        ? "found"
        : "not found",
      smoke.songs.some(
        (song) =>
          song.separation_status === "passed" &&
          song.vocals_path != null &&
          song.accompaniment_path != null,
      ),
      "",
    );

    await validateAudioOutputs(restart, result);
  }

  await validateRuntimeIdentity(restart, result);
  await validateModelIdentity(restart, result);

  const passCount = result.assertions.filter((a) => a.result === "pass").length;
  const failCount = result.assertions.filter((a) => a.result === "fail").length;
  appendSummary("");
  appendSummary(
    `**Total: ${passCount} passed, ${failCount} failed, ${result.assertions.length} assertions.**`,
  );

  return result;
}

async function main() {
  const [preparePath, restartPath, outputDir] = process.argv.slice(2);
  if (!preparePath || !restartPath) {
    throw new Error(
      "usage: node scripts/validate-installed-app-smoke.mjs <prepare-report> <restart-report> [output-dir]",
    );
  }

  const prepare = readReport(preparePath);
  const restart = readReport(restartPath);
  const result = await validateInstalledAppSmokeReports(prepare, restart);

  if (outputDir) {
    const validationReportPath = join(
      outputDir,
      "installed-app-smoke-validation.json",
    );
    writeFileSync(
      validationReportPath,
      JSON.stringify(
        {
          generated_at: Date.now(),
          prepare_report: preparePath,
          restart_report: restartPath,
          assertions: result.assertions,
          pass_count: result.assertions.filter((a) => a.result === "pass")
            .length,
          fail_count: result.assertions.filter((a) => a.result === "fail")
            .length,
        },
        null,
        2,
      ),
    );
    console.log(`Validation report written to ${validationReportPath}`);
  }

  if (result.failures.length > 0) {
    throw new Error(
      `Installed app release smoke failed:\n${result.failures.map((failure) => `- ${failure}`).join("\n")}`,
    );
  }

  console.log("Installed app release smoke passed.");
}

if (
  process.argv[1] &&
  resolve(fileURLToPath(import.meta.url)) === resolve(process.argv[1])
) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
