/**
 * Arbitrates who owns the lyrics viewport's scroll position.
 *
 * A resume request re-anchors the viewport on the playing line at the next
 * engine frame: callers observe the generation and treat any change as "snap
 * back now". Until that frame writes `scrollTop`, unlock suppression keeps the
 * scroll events the write itself provokes from being mistaken for the user
 * taking over again.
 */
export class LyricsScrollControl {
  private resumeGeneration = 0;
  private unlockSuppressed = false;

  requestResume(): void {
    this.resumeGeneration += 1;
    this.unlockSuppressed = true;
  }

  peekResumeGeneration(): number {
    return this.resumeGeneration;
  }

  isUnlockSuppressed(): boolean {
    return this.unlockSuppressed;
  }

  endUnlockSuppress(): void {
    this.unlockSuppressed = false;
  }
}
