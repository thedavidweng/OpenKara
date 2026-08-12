import { create, type StoreApi, type UseBoundStore } from "zustand";
import { splitCompanionRomanization } from "@/lib/lyrics-companion-romanization";
import {
  buildLyricsIdentity,
  type LocalAudienceRomanizeState,
} from "@/lib/local-audience-romanize";
import {
  findActiveLyricLineIndex,
  findActiveWordIndex,
} from "@/lib/lyrics-timing";
import type { LyricLine, LyricsSource } from "@/types/ipc";
import { LyricsScrollControl } from "./scroll-control";
import type {
  LyricsAlignment,
  LyricsData,
  LyricsSessionDependencies,
  LyricsState,
} from "./types";

/**
 * Lyrics the user or the catalog owner put there deliberately. Lyrics
 * Acquisition may still hand these back unsynced; replacing them with an
 * online guess would silently discard that intent, so automatic upgrade skips
 * them.
 */
const AUTO_UPGRADE_PROTECTED_SOURCES: ReadonlySet<LyricsSource> =
  new Set<LyricsSource>([
    "manual",
    "manual_ttml",
    "manual_lys",
    "sidecar",
    "sidecar_ttml",
    "sidecar_lys",
  ]);

const INITIAL_DATA: LyricsData = {
  songId: null,
  lines: [],
  source: null,
  offsetMs: 0,
  rawLrc: "",
  activeLineIndex: -1,
  activeWordIndex: -1,
  isLoading: false,
  romanizedLines: [],
  romanizedLinesIdentity: null,
  isRomanizing: false,
  showRomanized: false,
  lyricsAlignment: "left",
};

interface NormalizedLyrics {
  lines: LyricLine[];
  romanizedLines: string[];
  romanizedLinesIdentity: string | null;
}

function normalizeFetchedLyrics(lines: LyricLine[]): NormalizedLyrics {
  const split = splitCompanionRomanization(lines);
  return {
    lines: split.lines,
    romanizedLines: split.romanizedLines,
    romanizedLinesIdentity: split.complete
      ? buildLyricsIdentity(split.lines)
      : null,
  };
}

function isUnsynced(lines: LyricLine[]): boolean {
  return lines.length > 0 && lines.every((line) => line.time_ms === 0);
}

export type LyricsStore = UseBoundStore<StoreApi<LyricsState>>;

/**
 * Owns the lyrics of the song currently on the stage.
 *
 * Invariants the session keeps, so no caller has to:
 *
 * - **One winner per load.** Every `load` supersedes the one before it. A
 *   response that arrives after a newer load started is dropped, including the
 *   automatic-upgrade follow-up it may have queued.
 * - **Deliberate lyrics survive.** Automatic upgrade only runs for unsynced
 *   lyrics whose source is neither LRCLIB nor one of the protected sources,
 *   and only replaces them with an online result that is actually timed.
 * - **Lyrics Acquisition stays in the backend.** The session consumes the
 *   winning `LyricsPayload` and never re-runs the source chain itself.
 * - **One romanization at a time.** A romanization whose song changed under it
 *   is discarded, unless it came back without leaving the caller's turn.
 *   Results are cached against the identity of the lines they describe, so
 *   toggling the overlay never recomputes an unchanged transcription.
 * - **Offset is applied once.** Every position the session is asked about is
 *   converted through the same offset the backend persisted.
 * - **Active line and word only move forward through state.** Redundant
 *   updates never reach the store, so a frame loop can call the sync entries
 *   unconditionally.
 */
export class LyricsSession {
  /** React-facing projection: the data above plus the entries views call. */
  readonly store: LyricsStore;
  /** Who owns the viewport's scroll position right now. */
  readonly scroll = new LyricsScrollControl();

  private fetchGeneration = 0;
  private syncedLineIndex = -1;
  private syncedWordIndex = -1;

  constructor(private readonly deps: LyricsSessionDependencies) {
    this.store = create<LyricsState>(() => ({
      ...INITIAL_DATA,
      fetchLyrics: (songId) => this.load(songId),
      adjustOffset: (songId, deltaMs) => this.adjustOffset(songId, deltaMs),
      resetOffset: (songId) => this.resetOffset(songId),
      saveManualLyrics: (songId, text) => this.saveManualLyrics(songId, text),
      toggleRomanized: () => this.toggleRomanized(),
      setRomanizedVisibility: (show) => this.setRomanizedVisibility(show),
      applyRemoteRomanizeState: (state) => this.applyRemoteRomanizeState(state),
      toggleLyricsAlignment: () => this.toggleLyricsAlignment(),
      clear: () => this.clear(),
    }));
  }

  getState(): LyricsState {
    return this.store.getState();
  }

  subscribe(listener: (state: LyricsState) => void): () => void {
    return this.store.subscribe(listener);
  }

  async load(songId: string): Promise<void> {
    const generation = ++this.fetchGeneration;
    this.resetActiveIndexLatches();
    this.set({
      isLoading: true,
      lines: [],
      source: null,
      rawLrc: "",
      activeLineIndex: -1,
      activeWordIndex: -1,
      romanizedLines: [],
      romanizedLinesIdentity: null,
      showRomanized: false,
    });

    try {
      const payload = await this.deps.lyrics.fetchLyrics(songId);
      if (generation !== this.fetchGeneration) return;

      const normalized = normalizeFetchedLyrics(payload.lines);
      this.set({
        songId: payload.song_id,
        lines: normalized.lines,
        romanizedLines: normalized.romanizedLines,
        romanizedLinesIdentity: normalized.romanizedLinesIdentity,
        source: payload.source,
        offsetMs: payload.offset_ms,
        rawLrc: payload.raw_lrc,
        isLoading: false,
      });

      if (this.shouldAutoUpgrade(payload.source, normalized.lines)) {
        await this.autoUpgrade(songId, generation);
      }
    } catch (error) {
      if (generation !== this.fetchGeneration) return;
      this.deps.reportError(error);
      this.set({ lines: [], source: null, rawLrc: "", isLoading: false });
    }
  }

  clear(): void {
    this.resetActiveIndexLatches();
    this.set({
      songId: null,
      lines: [],
      source: null,
      offsetMs: 0,
      rawLrc: "",
      activeLineIndex: -1,
      activeWordIndex: -1,
      romanizedLines: [],
      romanizedLinesIdentity: null,
      isRomanizing: false,
      showRomanized: false,
    });
  }

  async setOffset(songId: string, ms: number): Promise<void> {
    await this.deps.lyrics.setLyricsOffset(songId, ms);
    this.set({ offsetMs: ms });
  }

  async adjustOffset(songId: string, deltaMs: number): Promise<void> {
    const nextOffset = this.getState().offsetMs + deltaMs;
    this.set({ offsetMs: nextOffset });

    try {
      await this.deps.lyrics.setLyricsOffset(songId, nextOffset);
    } catch (error) {
      await this.restorePersistedOffset(songId, deltaMs);
      this.deps.reportError(error);
    }
  }

  async resetOffset(songId: string): Promise<void> {
    const currentOffset = this.getState().offsetMs;
    if (currentOffset === 0) return;
    await this.adjustOffset(songId, -currentOffset);
  }

  async saveManualLyrics(songId: string, text: string): Promise<boolean> {
    try {
      const payload = await this.deps.lyrics.saveManualLyrics(songId, text);
      const normalized = normalizeFetchedLyrics(payload.lines);
      this.set({
        songId: payload.song_id,
        lines: normalized.lines,
        romanizedLines: normalized.romanizedLines,
        romanizedLinesIdentity: normalized.romanizedLinesIdentity,
        source: payload.source,
        offsetMs: payload.offset_ms,
        rawLrc: payload.raw_lrc,
      });
      return true;
    } catch (error) {
      this.deps.reportError(error);
      return false;
    }
  }

  readPositionMs(nowMs?: () => number): number {
    return this.deps.clock.readPositionMs(nowMs);
  }

  toAdjustedMs(positionMs: number): number {
    return positionMs - this.getState().offsetMs;
  }

  syncActiveLine(adjustedMs: number): void {
    const { lines } = this.getState();
    if (lines.length === 0) return;

    const index = findActiveLyricLineIndex(lines, adjustedMs);
    if (index === this.syncedLineIndex) return;

    this.syncedLineIndex = index;
    this.setActiveLineIndex(index);
  }

  syncActiveWord(adjustedMs: number): void {
    const { lines, activeLineIndex } = this.getState();
    const words = lines[activeLineIndex]?.words;

    if (words && words.length > 0) {
      const index = findActiveWordIndex(words, adjustedMs);
      if (index === this.syncedWordIndex) return;
      this.syncedWordIndex = index;
      this.setActiveWordIndex(index);
      return;
    }

    if (this.syncedWordIndex === -1) return;
    this.syncedWordIndex = -1;
    this.setActiveWordIndex(-1);
  }

  /** Forgets what was on screen, so the next frame re-derives from scratch. */
  resetActiveIndexLatches(): void {
    this.syncedLineIndex = -1;
    this.syncedWordIndex = -1;
  }

  toggleRomanized(): void {
    const { showRomanized, lines } = this.getState();
    if (lines.length === 0) return;
    this.setRomanizedVisibility(!showRomanized);
  }

  setRomanizedVisibility(show: boolean): void {
    const { showRomanized, lines, romanizedLines, romanizedLinesIdentity } =
      this.getState();
    if (lines.length === 0 || show === showRomanized) return;

    if (!show) {
      this.set({ showRomanized: false });
      return;
    }

    this.set({ showRomanized: true });
    if (
      buildLyricsIdentity(lines) !== romanizedLinesIdentity ||
      romanizedLines.length === 0
    ) {
      void this.romanizeCurrentLyrics();
    }
  }

  applyRemoteRomanizeState(state: LocalAudienceRomanizeState): void {
    this.set({
      showRomanized: state.showRomanized,
      isRomanizing: state.isRomanizing,
      romanizedLines: [...state.romanizedLines],
      romanizedLinesIdentity: state.lyricsIdentity,
    });
  }

  async romanizeCurrentLyrics(): Promise<void> {
    const { lines, isRomanizing, songId } = this.getState();
    if (isRomanizing || lines.length === 0) return;

    this.set({ isRomanizing: true });
    try {
      const { result, requestId } = await this.deps.romanization.romanize(
        lines.map((line) => line.text),
        this.deps.songLanguage.read(songId),
      );
      const answeredWithoutYielding = requestId === -1;
      if (!answeredWithoutYielding && this.getState().songId !== songId) {
        return;
      }
      this.set({
        romanizedLines: result,
        romanizedLinesIdentity: buildLyricsIdentity(this.getState().lines),
      });
    } catch (error) {
      console.error("Romanization failed:", error);
      this.set({ romanizedLines: [], romanizedLinesIdentity: null });
    } finally {
      this.set({ isRomanizing: false });
    }
  }

  /** Re-runs romanization when the song's catalog language changed under it. */
  refreshRomanization(): void {
    const { showRomanized, lines } = this.getState();
    if (!showRomanized || lines.length === 0) return;
    void this.romanizeCurrentLyrics();
  }

  setLyricsAlignment(alignment: LyricsAlignment): void {
    this.set({ lyricsAlignment: alignment });
  }

  toggleLyricsAlignment(): void {
    this.setLyricsAlignment(
      this.getState().lyricsAlignment === "left" ? "center" : "left",
    );
  }

  private set(patch: Partial<LyricsData>): void {
    this.store.setState(patch);
  }

  private setActiveLineIndex(index: number): void {
    if (index === this.getState().activeLineIndex) return;
    this.set({ activeLineIndex: index, activeWordIndex: -1 });
  }

  private setActiveWordIndex(index: number): void {
    if (index === this.getState().activeWordIndex) return;
    this.set({ activeWordIndex: index });
  }

  private shouldAutoUpgrade(
    source: LyricsSource | null,
    lines: LyricLine[],
  ): boolean {
    if (source === "lrc_lib") return false;
    if (source !== null && AUTO_UPGRADE_PROTECTED_SOURCES.has(source)) {
      return false;
    }
    return isUnsynced(lines);
  }

  private async autoUpgrade(songId: string, generation: number): Promise<void> {
    try {
      const online = await this.deps.lyrics.fetchLyricsOnline(
        songId,
        "automatic_upgrade",
      );
      if (
        generation !== this.fetchGeneration ||
        this.getState().songId !== songId ||
        online.lines.length === 0 ||
        !online.lines.some((line) => line.time_ms > 0)
      ) {
        return;
      }

      const normalized = normalizeFetchedLyrics(online.lines);
      this.set({
        songId: online.song_id,
        lines: normalized.lines,
        romanizedLines: normalized.romanizedLines,
        romanizedLinesIdentity: normalized.romanizedLinesIdentity,
        source: online.source,
        offsetMs: online.offset_ms,
        rawLrc: online.raw_lrc,
      });
    } catch {
      // The local result already on screen stays; offline is not an error.
    }
  }

  private async restorePersistedOffset(
    songId: string,
    deltaMs: number,
  ): Promise<void> {
    try {
      const payload = await this.deps.lyrics.fetchLyrics(songId);
      if (this.getState().songId === songId) {
        this.set({ offsetMs: payload.offset_ms });
      }
    } catch {
      if (this.getState().songId === songId) {
        this.set({ offsetMs: this.getState().offsetMs - deltaMs });
      }
    }
  }
}

export function createLyricsSession(
  deps: LyricsSessionDependencies,
): LyricsSession {
  return new LyricsSession(deps);
}
