/**
 * Manages per-word karaoke fill animations using Web Animations API.
 * Each word gets a CSS mask that sweeps from left to right over its duration.
 * Animations are driven by the browser's compositor, not React re-renders.
 *
 * Includes attack/release alpha smoothing for natural mask contrast transitions.
 */

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

  // Alpha smoothing state
  private brightAlpha = 0.2;
  private darkAlpha = 1.0;
  private targetBrightAlpha = 0.2;
  private targetDarkAlpha = 1.0;

  private static readonly ATTACK_SPEED = 50.0;
  private static readonly RELEASE_SPEED = 7.0;

  private lastUpdateTime = 0;

  /**
   * Set up animations for a line's word elements.
   * Call this when a line becomes active.
   */
  activateLine(
    lineEl: HTMLElement,
    words: Array<{ time_ms: number; end_ms: number }>,
    wordEls: HTMLElement[],
  ) {
    if (this.activeLineEl === lineEl) return;
    this.deactivateLine();
    this.activeLineEl = lineEl;

    for (let i = 0; i < words.length && i < wordEls.length; i++) {
      const word = words[i];
      const el = wordEls[i];
      const duration = Math.max(1, word.end_ms - word.time_ms);
      const style = el.style as WebKitMaskStyle;

      // Set up the mask gradient (static)
      this.setMaskGradient(style);
      style.maskRepeat = "no-repeat";
      style.webkitMaskRepeat = "no-repeat";
      style.maskOrigin = "left";
      style.webkitMaskOrigin = "left";
      style.maskSize = "200% 100%";
      style.webkitMaskSize = "200% 100%";

      // Create the sweep animation
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

    this.lastUpdateTime = 0;
  }

  /**
   * Set the target alpha values for the mask gradient.
   * bright: alpha for the "filled" portion (0-1). Active lines: 1.0, inactive: 0.2
   * dark: alpha for the "unfilled" portion (0-1). Typically 1.0
   */
  setTargetAlpha(bright: number, dark: number) {
    this.targetBrightAlpha = bright;
    this.targetDarkAlpha = dark;
  }

  /**
   * Update animation progress based on current playback time.
   * Call this on each frame (from requestAnimationFrame or React render).
   */
  update(currentMs: number, isPlaying: boolean) {
    // Compute dt for alpha smoothing
    const now = performance.now();
    const dt =
      this.lastUpdateTime > 0
        ? Math.min((now - this.lastUpdateTime) / 1000, 0.05)
        : 1 / 60;
    this.lastUpdateTime = now;

    // Smooth alpha with attack/release curves
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

    // Update mask gradient with smoothed alpha
    for (const [, wa] of this.wordAnimations) {
      this.setMaskGradient(wa.element.style as WebKitMaskStyle);
    }

    for (const [, wa] of this.wordAnimations) {
      if (currentMs < wa.startTime) {
        // Word hasn't started
        wa.animation.currentTime = 0;
        wa.animation.pause();
      } else if (currentMs >= wa.endTime) {
        // Word is done
        wa.animation.currentTime = wa.endTime - wa.startTime;
        wa.animation.pause();
      } else {
        // Word is active
        wa.animation.currentTime = currentMs - wa.startTime;
        if (isPlaying) {
          wa.animation.play();
        } else {
          wa.animation.pause();
        }
      }
    }
  }

  /**
   * Remove all animations and clean up.
   */
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
    const gradient = `linear-gradient(to right, rgba(0,0,0,${this.darkAlpha}), rgba(0,0,0,${this.brightAlpha}))`;
    style.maskImage = gradient;
    style.webkitMaskImage = gradient;
  }
}
