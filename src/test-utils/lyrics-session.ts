import type { SongLanguage } from "@/components/Library/song-list-item-menu";
import type { Backend } from "@/lib/backend";
import { createMockBackend } from "@/lib/backend/mock-backend";
import {
  createLyricsSession,
  type LyricsSession,
  type PlaybackClockPort,
  type RomanizationPort,
  type SongLanguagePort,
} from "@/lib/lyrics-session";

export interface RomanizeCall {
  lines: string[];
  language: SongLanguage | null;
}

/**
 * Romanization that answers in the caller's own microtask drain. `requestId`
 * mirrors the Worker adapter: a positive id can go stale, `-1` marks a result
 * the caller must treat as never stale.
 */
export class FakeRomanization implements RomanizationPort {
  readonly calls: RomanizeCall[] = [];

  private nextRequestId = 1;
  private transform: (line: string) => string = (line) => `roman(${line})`;
  private failure: unknown = null;
  private withoutYielding = false;
  private holdNext = false;
  private held: {
    lines: string[];
    resolve: (value: { result: string[]; requestId: number }) => void;
    reject: (error: unknown) => void;
  } | null = null;

  respondWith(transform: (line: string) => string): this {
    this.transform = transform;
    this.failure = null;
    return this;
  }

  failWith(error: unknown): this {
    this.failure = error;
    return this;
  }

  /** Makes every later answer carry the never-stale `-1` request id. */
  answerWithoutYielding(): this {
    this.withoutYielding = true;
    return this;
  }

  /** Holds the next `romanize` call until `release()` so a mid-flight upgrade can land. */
  hold(): this {
    this.holdNext = true;
    return this;
  }

  release(): void {
    const held = this.held;
    this.held = null;
    this.holdNext = false;
    if (!held) return;
    if (this.failure !== null) {
      held.reject(this.failure);
      return;
    }
    held.resolve({
      result: held.lines.map(this.transform),
      requestId: this.withoutYielding ? -1 : this.nextRequestId++,
    });
  }

  async romanize(
    lines: readonly string[],
    language: SongLanguage | null,
  ): Promise<{ result: string[]; requestId: number }> {
    this.calls.push({ lines: [...lines], language });
    if (this.holdNext) {
      this.holdNext = false;
      return new Promise((resolve, reject) => {
        this.held = { lines: [...lines], resolve, reject };
      });
    }
    if (this.failure !== null) {
      throw this.failure;
    }
    return {
      result: lines.map(this.transform),
      requestId: this.withoutYielding ? -1 : this.nextRequestId++,
    };
  }
}

export class FakeClock implements PlaybackClockPort {
  constructor(public positionMs = 0) {}

  private source: (() => number) | null = null;

  /** Reads from a live source instead of the mutable `positionMs` field. */
  readFrom(source: () => number): this {
    this.source = source;
    return this;
  }

  readPositionMs(): number {
    return this.source ? this.source() : this.positionMs;
  }
}

export interface TestLyricsSessionOptions {
  backend?: Backend;
  songLanguage?: SongLanguagePort;
  reportError?: (error: unknown) => void;
}

export interface TestLyricsSession {
  session: LyricsSession;
  backend: Backend;
  romanization: FakeRomanization;
  clock: FakeClock;
  errors: unknown[];
}

/**
 * A `LyricsSession` wired to the shared in-memory backend fake, so tests drive
 * the same interface the app does instead of the store's internals.
 */
export function createTestLyricsSession(
  options: TestLyricsSessionOptions = {},
): TestLyricsSession {
  const backend = options.backend ?? createMockBackend();
  const romanization = new FakeRomanization();
  const clock = new FakeClock();
  const errors: unknown[] = [];

  const session = createLyricsSession({
    lyrics: backend.lyrics,
    romanization,
    songLanguage: options.songLanguage ?? { read: () => null },
    clock,
    reportError: options.reportError ?? ((error) => errors.push(error)),
  });

  return { session, backend, romanization, clock, errors };
}
