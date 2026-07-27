import { KaraokeFillController } from "@/components/Lyrics/karaoke-fill";
import { Spring } from "@/lib/spring";

const LINE_SPRING_SCALE = { stiffness: 96, damping: 22 };
const LINE_SPRING_OPACITY = { stiffness: 80, damping: 20 };
const LINE_SPRING_BLUR = { stiffness: 80, damping: 20 };

function getLineVisualTargets(distance: number): {
  targetScale: number;
  targetOpacity: number;
} {
  const targetScale =
    distance === 0
      ? 1
      : distance === 1
        ? 0.98
        : Math.max(0.94, 1 - distance * 0.018);
  const targetOpacity =
    distance === 0 ? 1 : Math.max(0.38, 1 - distance * 0.16);
  return { targetScale, targetOpacity };
}

interface RegisteredLineWrapper {
  wrapperEl: HTMLElement | null;
  scale: Spring;
  opacity: Spring;
  blur: Spring;
}

export class LyricsLineRuntime {
  private wrappers = new Map<number, RegisteredLineWrapper>();
  private karaokeByLine = new Map<number, KaraokeFillController>();

  registerWrapper(lineIndex: number, el: HTMLElement): void {
    const existing = this.wrappers.get(lineIndex);
    if (existing) {
      existing.wrapperEl = el;
      return;
    }

    this.wrappers.set(lineIndex, {
      wrapperEl: el,
      scale: new Spring(1, LINE_SPRING_SCALE),
      opacity: new Spring(1, LINE_SPRING_OPACITY),
      blur: new Spring(0, LINE_SPRING_BLUR),
    });
  }

  unregisterWrapper(lineIndex: number): void {
    const existing = this.wrappers.get(lineIndex);
    if (existing) {
      existing.wrapperEl = null;
    }
  }

  registerKaraoke(lineIndex: number, controller: KaraokeFillController): void {
    this.karaokeByLine.set(lineIndex, controller);
  }

  unregisterKaraoke(lineIndex: number): void {
    this.karaokeByLine.delete(lineIndex);
  }

  clear(): void {
    this.wrappers.clear();
    this.karaokeByLine.clear();
  }

  tick(input: {
    activeLineIndex: number;
    adjustedMs: number;
    isPlaying: boolean;
    dt: number;
    isPlainText: boolean;
  }): void {
    if (input.isPlainText) {
      return;
    }

    for (const [index, entry] of this.wrappers) {
      const distance = Math.abs(index - input.activeLineIndex);
      const { targetScale, targetOpacity } = getLineVisualTargets(distance);
      entry.scale.setTarget(targetScale);
      entry.opacity.setTarget(targetOpacity);
      entry.blur.setTarget(0);
      entry.scale.update(input.dt);
      entry.opacity.update(input.dt);
      entry.blur.update(input.dt);

      const wrapperEl = entry.wrapperEl;
      if (!wrapperEl) {
        continue;
      }

      wrapperEl.style.transform = `scale(${entry.scale.getPosition().toFixed(4)})`;
      wrapperEl.style.opacity = String(entry.opacity.getPosition());
      wrapperEl.style.filter = `blur(${entry.blur.getPosition().toFixed(1)}px)`;
      wrapperEl.style.willChange = "transform, opacity, filter";
      wrapperEl.style.contain = "layout style paint";
    }

    const karaoke = this.karaokeByLine.get(input.activeLineIndex);
    karaoke?.update(input.adjustedMs, input.isPlaying);
  }
}

export const lyricsLineRuntime = new LyricsLineRuntime();
