import { KaraokeFillController } from "@/components/Lyrics/karaoke-fill";
import {
  canUseMeasuredFocusLayout,
  FOCUS_LINE_GAP_PX,
  focusHeadPadPx,
  layoutFocusLineTops,
} from "@/lib/lyrics-focus-layout";
import { Spring } from "@/lib/spring";

const LINE_SPRING_SCALE = { stiffness: 88, damping: 20 };
const LINE_SPRING_OPACITY = { stiffness: 76, damping: 20 };
const LINE_SPRING_BLUR = { stiffness: 76, damping: 20 };

export type LyricsStage = "focus" | "list";

export function getLineVisualTargets(
  distance: number,
  stage: LyricsStage = "list",
): {
  targetScale: number;
  targetOpacity: number;
  targetBlur: number;
} {
  if (distance === 0) {
    return { targetScale: 1, targetOpacity: 0.85, targetBlur: 0 };
  }

  const blur = Math.min(5, 1 + distance);
  if (stage === "focus") {
    return { targetScale: 0.97, targetOpacity: 1, targetBlur: blur };
  }

  const targetScale =
    distance === 1 ? 0.98 : Math.max(0.94, 1 - distance * 0.018);
  return { targetScale, targetOpacity: 1, targetBlur: blur };
}

interface RegisteredLineWrapper {
  wrapperEl: HTMLElement | null;
  scale: Spring;
  opacity: Spring;
  blur: Spring;
  top: Spring;
  hasFocusOrigin: boolean;
}

export class LyricsLineRuntime {
  private wrappers = new Map<number, RegisteredLineWrapper>();
  private karaokeByLine = new Map<number, KaraokeFillController>();
  private lastStageEl: HTMLElement | null = null;

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
      top: new Spring(0, LINE_SPRING_SCALE),
      hasFocusOrigin: false,
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
    stage?: LyricsStage;
    viewportEl?: HTMLElement | null;
  }): void {
    if (input.isPlainText) {
      return;
    }

    const stage = input.stage ?? "list";
    const origin = stage === "focus" ? "center center" : "left center";
    const placed = this.placeFocusSlots(stage, input.viewportEl, input.dt);

    for (const [index, entry] of this.wrappers) {
      const distance = Math.abs(index - input.activeLineIndex);
      const { targetScale, targetOpacity, targetBlur } = getLineVisualTargets(
        distance,
        stage,
      );
      entry.scale.setTarget(targetScale);
      entry.opacity.setTarget(targetOpacity);
      entry.blur.setTarget(targetBlur);
      entry.scale.update(input.dt);
      entry.opacity.update(input.dt);
      entry.blur.update(input.dt);

      const wrapperEl = entry.wrapperEl;
      if (!wrapperEl) {
        continue;
      }

      const blur = entry.blur.getPosition();
      wrapperEl.style.transformOrigin = origin;
      wrapperEl.style.transform = `scale(${entry.scale.getPosition().toFixed(4)})`;
      wrapperEl.style.opacity = String(entry.opacity.getPosition());
      wrapperEl.style.filter =
        blur > 0.05 ? `blur(${blur.toFixed(2)}px)` : "none";
      wrapperEl.style.willChange = placed
        ? "transform, opacity, filter, top"
        : "transform, opacity, filter";
      wrapperEl.style.contain = "";
    }

    const karaoke = this.karaokeByLine.get(input.activeLineIndex);
    karaoke?.update(input.adjustedMs, input.isPlaying);
  }

  private placeFocusSlots(
    stage: LyricsStage,
    viewportEl: HTMLElement | null | undefined,
    dt: number,
  ): boolean {
    const stageEl =
      viewportEl?.querySelector<HTMLElement>('[data-lyrics-stage="focus"]') ??
      null;

    if (stage !== "focus" || !stageEl) {
      this.clearFocusSlots();
      return false;
    }

    const ordered = [...this.wrappers.entries()].sort(([a], [b]) => a - b);
    const heights = ordered.map(
      ([, entry]) => entry.wrapperEl?.offsetHeight ?? 0,
    );
    if (!canUseMeasuredFocusLayout(heights)) {
      this.clearFocusSlots();
      return false;
    }

    const { tops, stageHeight } = layoutFocusLineTops(
      heights,
      FOCUS_LINE_GAP_PX,
      focusHeadPadPx(viewportEl?.clientHeight ?? 0),
    );
    this.lastStageEl = stageEl;
    stageEl.style.position = "relative";
    stageEl.style.height = `${stageHeight}px`;

    ordered.forEach(([, entry], index) => {
      const wrapperEl = entry.wrapperEl;
      if (!wrapperEl) {
        return;
      }
      const targetTop = tops[index] ?? 0;
      if (!entry.hasFocusOrigin) {
        entry.top.jumpTo(targetTop);
        entry.hasFocusOrigin = true;
      } else {
        entry.top.setTarget(targetTop);
        entry.top.update(dt);
      }
      wrapperEl.style.position = "absolute";
      wrapperEl.style.left = "0";
      wrapperEl.style.width = "100%";
      wrapperEl.style.top = `${entry.top.getPosition().toFixed(1)}px`;
    });

    return true;
  }

  private clearFocusSlots(): void {
    if (this.lastStageEl) {
      this.lastStageEl.style.height = "";
      this.lastStageEl.style.position = "";
      this.lastStageEl = null;
    }
    for (const entry of this.wrappers.values()) {
      entry.hasFocusOrigin = false;
      const wrapperEl = entry.wrapperEl;
      if (!wrapperEl) {
        continue;
      }
      wrapperEl.style.position = "";
      wrapperEl.style.left = "";
      wrapperEl.style.width = "";
      wrapperEl.style.top = "";
      wrapperEl.style.contain = "";
    }
  }
}

export const lyricsLineRuntime = new LyricsLineRuntime();
