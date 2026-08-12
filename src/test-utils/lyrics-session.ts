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
 * Romanization that answers on the caller's next microtask. `requestId` mirrors
 * the Worker adapter: a positive id can go stale, `-1` marks a result produced
 * without leaving the caller's turn.
 */
export class FakeRomanization implements RomanizationPort {
  readonly calls: RomanizeCall[] = [];

  private nextRequestId = 1;
  private transform: (line: string) => string = (line) => `roman(${line})`;
  private failure: unknown = null;
  private withoutYielding = false;

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

  async romanize(
    lines: readonly string[],
    language: SongLanguage | null,
  ): Promise<{ result: string[]; requestId: number }> {
    this.calls.push({ lines: [...lines], language });
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

  readPositionMs(): number {
    return this.positionMs;
  }
}

export interface TestLyricsSessionOptions {
  backend?: Backend;
  romanization?: RomanizationPort;
  songLanguage?: SongLanguagePort;
  clock?: PlaybackClockPort;
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
    romanization: options.romanization ?? romanization,
    songLanguage: options.songLanguage ?? { read: () => null },
    clock: options.clock ?? clock,
    reportError: options.reportError ?? ((error) => errors.push(error)),
  });

  return { session, backend, romanization, clock, errors };
}
