interface WordAnimation {
  element: HTMLElement;
  animation: Animation;
  startTime: number;
  endTime: number;
}

type WebKitMaskStyle = CSSStyleDeclaration & {
  webkitMaskImage: string;
  webkitMaskRepeat: string;
  webkitMaskOrigin: string;
  webkitMaskSize: string;
};

export class KaraokeFillController {
  private wordAnimations = new Map<HTMLElement, WordAnimation>();
  private activeLineEl: HTMLElement | null = null;
  private activeWordEls: HTMLElement[] = [];
  private activeWordTimings: Array<{ time_ms: number; end_ms: number }> = [];

  private brightAlpha = 0.2;
  private darkAlpha = 1.0;
  private targetBrightAlpha = 0.2;
  private targetDarkAlpha = 1.0;

  private static readonly ATTACK_SPEED = 50.0;
  private static readonly RELEASE_SPEED = 7.0;

  private lastUpdateTime = 0;

  activateLine(
    lineEl: HTMLElement,
    words: Array<{ time_ms: number; end_ms: number }>,
    wordEls: HTMLElement[],
  ) {
    if (this.activeLineEl === lineEl && this.hasSameBinding(words, wordEls)) {
      return;
    }
    this.deactivateLine();
    this.activeLineEl = lineEl;

    for (let i = 0; i < words.length && i < wordEls.length; i++) {
      const word = words[i];
      const el = wordEls[i];
      const duration = Math.max(1, word.end_ms - word.time_ms);
      const style = el.style as WebKitMaskStyle;

      this.setMaskGradient(style);
      style.maskRepeat = "no-repeat";
      style.webkitMaskRepeat = "no-repeat";
      style.maskOrigin = "left";
      style.webkitMaskOrigin = "left";
      style.maskSize = "200% 100%";
      style.webkitMaskSize = "200% 100%";

      const animation = el.animate(
        [
          { maskPosition: "-100% 0", webkitMaskPosition: "-100% 0" },
          { maskPosition: "0% 0", webkitMaskPosition: "0% 0" },
        ],
        {
          duration,
          fill: "forwards",
          easing: "linear",
        },
      );
      animation.pause();

      this.wordAnimations.set(el, {
        element: el,
        animation,
        startTime: word.time_ms,
        endTime: word.end_ms,
      });
    }
    this.activeWordEls = wordEls.slice(0, this.wordAnimations.size);
    this.activeWordTimings = words.slice(0, this.wordAnimations.size);

    this.lastUpdateTime = 0;
  }

  setTargetAlpha(bright: number, dark: number) {
    this.targetBrightAlpha = bright;
    this.targetDarkAlpha = dark;
  }

  setCurrentAlpha(bright: number, dark: number) {
    this.brightAlpha = bright;
    this.darkAlpha = dark;
    this.setTargetAlpha(bright, dark);
    this.updateMaskGradients();
  }

  update(currentMs: number, isPlaying: boolean) {
    const now = performance.now();
    const dt =
      this.lastUpdateTime > 0
        ? Math.min((now - this.lastUpdateTime) / 1000, 0.05)
        : 1 / 60;
    this.lastUpdateTime = now;

    const brightSpeed =
      this.targetBrightAlpha > this.brightAlpha
        ? KaraokeFillController.ATTACK_SPEED
        : KaraokeFillController.RELEASE_SPEED;
    const darkSpeed =
      this.targetDarkAlpha > this.darkAlpha
        ? KaraokeFillController.ATTACK_SPEED
        : KaraokeFillController.RELEASE_SPEED;

    this.brightAlpha +=
      (this.targetBrightAlpha - this.brightAlpha) *
      (1 - Math.exp(-brightSpeed * dt));
    this.darkAlpha +=
      (this.targetDarkAlpha - this.darkAlpha) * (1 - Math.exp(-darkSpeed * dt));

    this.updateMaskGradients();

    for (const [, wa] of this.wordAnimations) {
      if (currentMs < wa.startTime) {
        wa.animation.currentTime = 0;
        wa.animation.pause();
      } else if (currentMs >= wa.endTime) {
        wa.animation.currentTime = wa.endTime - wa.startTime;
        wa.animation.pause();
      } else {
        wa.animation.currentTime = currentMs - wa.startTime;
        if (isPlaying) {
          wa.animation.play();
        } else {
          wa.animation.pause();
        }
      }
    }
  }

  deactivateLine() {
    for (const [, wa] of this.wordAnimations) {
      wa.animation.cancel();
      const style = wa.element.style as WebKitMaskStyle;
      style.maskImage = "";
      style.webkitMaskImage = "";
      style.maskRepeat = "";
      style.webkitMaskRepeat = "";
      style.maskOrigin = "";
      style.webkitMaskOrigin = "";
      style.maskSize = "";
      style.webkitMaskSize = "";
    }
    this.wordAnimations.clear();
    this.activeLineEl = null;
    this.activeWordEls = [];
    this.activeWordTimings = [];
    this.brightAlpha = 0.2;
    this.darkAlpha = 1.0;
    this.targetBrightAlpha = 0.2;
    this.targetDarkAlpha = 1.0;
    this.lastUpdateTime = 0;
  }

  destroy() {
    this.deactivateLine();
  }

  private setMaskGradient(style: WebKitMaskStyle) {
    const gradient = `linear-gradient(to right, rgba(0,0,0,${this.brightAlpha}), rgba(0,0,0,${this.darkAlpha}))`;
    style.maskImage = gradient;
    style.webkitMaskImage = gradient;
  }

  private updateMaskGradients() {
    for (const [, wa] of this.wordAnimations) {
      this.setMaskGradient(wa.element.style as WebKitMaskStyle);
    }
  }

  private hasSameBinding(
    words: Array<{ time_ms: number; end_ms: number }>,
    wordEls: HTMLElement[],
  ) {
    const bindingLength = Math.min(words.length, wordEls.length);
    if (bindingLength !== this.activeWordEls.length) {
      return false;
    }

    for (let index = 0; index < bindingLength; index += 1) {
      const activeTiming = this.activeWordTimings[index];
      const nextTiming = words[index];
      if (
        this.activeWordEls[index] !== wordEls[index] ||
        activeTiming.time_ms !== nextTiming.time_ms ||
        activeTiming.end_ms !== nextTiming.end_ms
      ) {
        return false;
      }
    }

    return true;
  }
}
