import { useState, useCallback, useMemo, useRef, useEffect } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import {
  Mic2,
  Music,
  ChevronDown,
  Drum,
  Guitar,
  AudioWaveform,
  SlidersHorizontal,
  type LucideIcon,
} from "lucide-react";
import { Tooltip } from "@/components/Overlay/Tooltip";
import { AudioLevelSlider } from "./AudioLevelSlider";
import {
  createTrailingRateLimiter,
  type TrailingRateLimiter,
} from "@/lib/rate-limit";
import { usePlayerStore } from "@/stores/player-store";
import { useLibraryStore } from "@/stores/library-store";
import type { StemName } from "@/types/ipc";
import {
  getPlaybackBarLayoutTokens,
  type PlaybackBarDensity,
} from "./playback-bar-layout";

const STEM_POPUP_GAP_PX = 12;
/**
 * Popup surface uses Tailwind `p-4` (16px). Position so the *content* icon
 * column lines up with the accompaniment mute button's left edge:
 *   panel.left = muteButton.left - PAD
 */
const STEM_POPUP_PAD_PX = 16;
/**
 * Extra trailing margin on popup rails. Icons sit inside a 44px hit target so
 * the glyph has ~13px inset; the range thumb sits at the rail's end. Equal
 * box padding alone still looks right-heavy — this restores the pre-portal
 * `mr-[14px]` optical balance between left glyph inset and right thumb gap.
 */
const STEM_POPUP_RAIL_TRAIL_CLASS = "mr-[14px]";

interface VolumeSlidersProps {
  density?: PlaybackBarDensity;
}

export function VolumeSliders({
  density = "relaxed",
}: VolumeSlidersProps = {}) {
  const { t } = useTranslation();
  const snapshot = usePlayerStore((s) => s.snapshot);
  const setStemVolume = usePlayerStore((s) => s.setStemVolume);
  const separationStatuses = useLibraryStore((s) => s.separationStatuses);

  const [isExpanded, setIsExpanded] = useState(false);
  const throttledSetStemVolumeRef = useRef(
    new Map<StemName, TrailingRateLimiter<number>>(),
  );
  const accompGroupLimiterRef = useRef<TrailingRateLimiter<{
    drums: number;
    bass: number;
    other: number;
  }> | null>(null);

  useEffect(() => {
    throttledSetStemVolumeRef.current.forEach((limiter) => limiter.cancel());
    throttledSetStemVolumeRef.current = new Map();
    accompGroupLimiterRef.current?.cancel();
    accompGroupLimiterRef.current = null;
    return () => {
      throttledSetStemVolumeRef.current.forEach((limiter) => limiter.cancel());
      accompGroupLimiterRef.current?.cancel();
    };
  }, [setStemVolume]);

  const stemVolumes = useMemo(
    () =>
      snapshot?.stem_volumes ?? {
        vocals: 1,
        drums: 1,
        bass: 1,
        other: 1,
      },
    [snapshot?.stem_volumes],
  );
  const hasStems = snapshot?.has_stems ?? false;
  const stemMode = snapshot?.stem_mode ?? null;
  const songId = snapshot?.song_id;
  const isSeparated =
    songId != null && separationStatuses[songId]?.state === "completed";
  const stemsAvailable = hasStems && isSeparated;
  const isTwoStem = stemMode === "two_stem";
  const isFourStem = stemMode === "four_stem";

  const prevVocalsRef = useRef(1);
  const prevAccompRef = useRef({ drums: 1, bass: 1, other: 1 });
  const prevDrumsRef = useRef(1);
  const prevBassRef = useRef(1);
  const prevOtherRef = useRef(1);

  const accompGestureBaseRef = useRef<{
    drums: number;
    bass: number;
    other: number;
    master: number;
  } | null>(null);
  const [accompDragValue, setAccompDragValue] = useState<number | null>(null);

  // Floating popup: portal to body so stage overflow / settings z-index cannot clip it.
  const popupRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  /**
   * Anchors to the accompaniment *mute button* (not the whole row), so the
   * popup icon column lines up with the Music control on the playback bar.
   */
  const accompMuteButtonRef = useRef<HTMLButtonElement>(null);
  /** Tight-density mixer trigger doubles as the position anchor. */
  const tightAnchorRef = useRef<HTMLDivElement>(null);
  const [popupPos, setPopupPos] = useState<{
    left: number;
    bottom: number;
  } | null>(null);

  const updatePopupPosition = useCallback(() => {
    const muteButton = accompMuteButtonRef.current;
    const tightAnchor = tightAnchorRef.current;
    const anchor = muteButton ?? tightAnchor;
    if (!anchor) {
      return;
    }
    const rect = anchor.getBoundingClientRect();
    setPopupPos({
      // Align popup content icons with the mute button: subtract surface pad.
      left: rect.left - STEM_POPUP_PAD_PX,
      bottom: window.innerHeight - rect.top + STEM_POPUP_GAP_PX,
    });
  }, []);

  useEffect(() => {
    if (!isExpanded) {
      setPopupPos(null);
      return;
    }
    updatePopupPosition();
    window.addEventListener("resize", updatePopupPosition);
    // Capture scroll from nested stage/settings panes.
    window.addEventListener("scroll", updatePopupPosition, true);
    return () => {
      window.removeEventListener("resize", updatePopupPosition);
      window.removeEventListener("scroll", updatePopupPosition, true);
    };
  }, [isExpanded, updatePopupPosition]);

  useEffect(() => {
    if (!isExpanded) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (
        popupRef.current &&
        !popupRef.current.contains(e.target as Node) &&
        triggerRef.current &&
        !triggerRef.current.contains(e.target as Node) &&
        tightAnchorRef.current &&
        !tightAnchorRef.current.contains(e.target as Node)
      ) {
        setIsExpanded(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [isExpanded]);

  const handleStemChange = useCallback(
    (stem: StemName, value: number) => {
      let limiter = throttledSetStemVolumeRef.current.get(stem);
      if (!limiter) {
        limiter = createTrailingRateLimiter(
          (nextValue: number) => setStemVolume(stem, nextValue),
          20,
        );
        throttledSetStemVolumeRef.current.set(stem, limiter);
      }
      limiter(value);
    },
    [setStemVolume],
  );

  // Accompaniment resting display value = max of the three sub-stems
  const accompValue = Math.max(
    stemVolumes.drums,
    stemVolumes.bass,
    stemVolumes.other,
  );

  const dispatchAccompGroup = useCallback(
    (drums: number, bass: number, other: number) => {
      let limiter = accompGroupLimiterRef.current;
      if (!limiter) {
        limiter = createTrailingRateLimiter(
          (triple: { drums: number; bass: number; other: number }) => {
            setStemVolume("drums", triple.drums);
            setStemVolume("bass", triple.bass);
            setStemVolume("other", triple.other);
          },
          20,
        );
        accompGroupLimiterRef.current = limiter;
      }
      limiter({ drums, bass, other });
    },
    [setStemVolume],
  );

  const handleAccompDragStart = useCallback(() => {
    if (accompGestureBaseRef.current) return;
    const { drums, bass, other } = stemVolumes;
    accompGestureBaseRef.current = {
      drums,
      bass,
      other,
      master: Math.max(drums, bass, other),
    };
  }, [stemVolumes]);

  const handleAccompDragEnd = useCallback(() => {
    accompGroupLimiterRef.current?.flush();
    accompGestureBaseRef.current = null;
  }, []);

  useEffect(() => {
    if (stemsAvailable) {
      return;
    }
    accompGroupLimiterRef.current?.cancel();
    accompGestureBaseRef.current = null;
    setAccompDragValue(null);
  }, [stemsAvailable]);

  useEffect(() => {
    accompGroupLimiterRef.current?.cancel();
    accompGestureBaseRef.current = null;
    setAccompDragValue(null);
  }, [songId]);

  // Hand the display back to the store once it catches up with the gesture.
  useEffect(() => {
    if (accompGestureBaseRef.current !== null) {
      return;
    }
    setAccompDragValue(null);
  }, [accompValue]);

  const handleAccompChange = useCallback(
    (newValue: number) => {
      const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
      const gestureBase = accompGestureBaseRef.current;
      if (gestureBase) {
        setAccompDragValue(newValue);
      }
      if (isTwoStem) {
        // In 2-stem mode, set all three sub-stems to the same value;
        // the backend uses max gain as the accompaniment gain.
        dispatchAccompGroup(newValue, newValue, newValue);
        return;
      }
      const base = gestureBase ?? {
        drums: stemVolumes.drums,
        bass: stemVolumes.bass,
        other: stemVolumes.other,
        master: accompValue,
      };
      if (base.master === 0) {
        dispatchAccompGroup(newValue, newValue, newValue);
        return;
      }
      const factor = newValue / base.master;
      dispatchAccompGroup(
        clamp01(base.drums * factor),
        clamp01(base.bass * factor),
        clamp01(base.other * factor),
      );
    },
    [isTwoStem, accompValue, stemVolumes, dispatchAccompGroup],
  );

  const handleVocalsMuteToggle = useCallback(() => {
    if (stemVolumes.vocals > 0) {
      prevVocalsRef.current = stemVolumes.vocals;
      setStemVolume("vocals", 0);
    } else {
      setStemVolume("vocals", prevVocalsRef.current);
    }
  }, [stemVolumes.vocals, setStemVolume]);

  const handleAccompMuteToggle = useCallback(() => {
    if (accompValue > 0) {
      prevAccompRef.current = {
        drums: stemVolumes.drums,
        bass: stemVolumes.bass,
        other: stemVolumes.other,
      };
      setStemVolume("drums", 0);
      setStemVolume("bass", 0);
      setStemVolume("other", 0);
    } else {
      const prev = prevAccompRef.current;
      setStemVolume("drums", prev.drums);
      setStemVolume("bass", prev.bass);
      setStemVolume("other", prev.other);
    }
  }, [accompValue, stemVolumes, setStemVolume]);

  const handleDrumsMuteToggle = useCallback(() => {
    if (stemVolumes.drums > 0) {
      prevDrumsRef.current = stemVolumes.drums;
      setStemVolume("drums", 0);
    } else {
      setStemVolume("drums", prevDrumsRef.current);
    }
  }, [stemVolumes.drums, setStemVolume]);

  const handleBassMuteToggle = useCallback(() => {
    if (stemVolumes.bass > 0) {
      prevBassRef.current = stemVolumes.bass;
      setStemVolume("bass", 0);
    } else {
      setStemVolume("bass", prevBassRef.current);
    }
  }, [stemVolumes.bass, setStemVolume]);

  const handleOtherMuteToggle = useCallback(() => {
    if (stemVolumes.other > 0) {
      prevOtherRef.current = stemVolumes.other;
      setStemVolume("other", 0);
    } else {
      setStemVolume("other", prevOtherRef.current);
    }
  }, [stemVolumes.other, setStemVolume]);

  const layoutTokens = getPlaybackBarLayoutTokens(density);
  const inlineSliderWidthClass = layoutTokens.inlineStemVolumeWidthClass;
  /*
   * Popup rails: longer master-rail token + trailing optical margin. The
   * trail margin is what makes left/right look equal — box p-4 alone is not
   * enough because icon glyphs inset inside 44px buttons while thumbs sit
   * flush on the rail end. Inline accompaniment keeps the stem token only.
   */
  const popupSliderWidthClass = `${layoutTokens.masterVolumeWidthClass} ${STEM_POPUP_RAIL_TRAIL_CLASS}`;
  const collapsedMode = density === "tight";
  const triggerLabel = isExpanded
    ? t("stems.collapseStems")
    : t("stems.expandStems");
  const canExpandFourStem = stemsAvailable && isFourStem;
  const canExpandTight = stemsAvailable && collapsedMode;
  const showPopup =
    isExpanded &&
    popupPos != null &&
    typeof document !== "undefined" &&
    (canExpandFourStem || canExpandTight);

  const popupSurfaceClassName =
    "stem-popup app-panel-surface fixed z-[70] w-max rounded-lg border border-[color-mix(in_srgb,var(--color-border)_85%,transparent)] bg-[color-mix(in_srgb,var(--color-sidebar)_94%,transparent)] p-4 shadow-[0_20px_40px_rgba(0,0,0,0.32)]";

  const fourStemPopupRows = (
    <div className="flex flex-col gap-2">
      <StemSlider
        icon={Drum}
        label={t("stems.drums")}
        value={stemVolumes.drums}
        onChange={(v) => handleStemChange("drums", v)}
        onIconClick={handleDrumsMuteToggle}
        sliderWidthClass={popupSliderWidthClass}
        iconButtonVariant="playback_bar"
      />
      <StemSlider
        icon={Guitar}
        label={t("stems.bass")}
        value={stemVolumes.bass}
        onChange={(v) => handleStemChange("bass", v)}
        onIconClick={handleBassMuteToggle}
        sliderWidthClass={popupSliderWidthClass}
        iconButtonVariant="playback_bar"
      />
      <StemSlider
        icon={AudioWaveform}
        label={t("stems.other")}
        value={stemVolumes.other}
        onChange={(v) => handleStemChange("other", v)}
        onIconClick={handleOtherMuteToggle}
        sliderWidthClass={popupSliderWidthClass}
        iconButtonVariant="playback_bar"
      />
    </div>
  );

  const tightPopupRows = (
    <div className="flex flex-col gap-2">
      <StemSlider
        icon={Mic2}
        label={t("stems.vocals")}
        value={stemVolumes.vocals}
        onChange={(v) => handleStemChange("vocals", v)}
        onIconClick={handleVocalsMuteToggle}
        sliderWidthClass={popupSliderWidthClass}
        iconButtonVariant="playback_bar"
        playbackActionName="vocals-mute"
      />
      <StemSlider
        icon={Music}
        label={t("stems.accompaniment")}
        value={accompDragValue ?? accompValue}
        onChange={handleAccompChange}
        onDragStart={handleAccompDragStart}
        onDragEnd={handleAccompDragEnd}
        onIconClick={handleAccompMuteToggle}
        sliderWidthClass={popupSliderWidthClass}
        iconButtonVariant="playback_bar"
        playbackActionName="accompaniment-mute"
      />
      {isFourStem ? (
        <>
          <div className="h-px bg-[color-mix(in_srgb,var(--color-border)_85%,transparent)]" />
          <StemSlider
            icon={Drum}
            label={t("stems.drums")}
            value={stemVolumes.drums}
            onChange={(v) => handleStemChange("drums", v)}
            onIconClick={handleDrumsMuteToggle}
            sliderWidthClass={popupSliderWidthClass}
            iconButtonVariant="playback_bar"
          />
          <StemSlider
            icon={Guitar}
            label={t("stems.bass")}
            value={stemVolumes.bass}
            onChange={(v) => handleStemChange("bass", v)}
            onIconClick={handleBassMuteToggle}
            sliderWidthClass={popupSliderWidthClass}
            iconButtonVariant="playback_bar"
          />
          <StemSlider
            icon={AudioWaveform}
            label={t("stems.other")}
            value={stemVolumes.other}
            onChange={(v) => handleStemChange("other", v)}
            onIconClick={handleOtherMuteToggle}
            sliderWidthClass={popupSliderWidthClass}
            iconButtonVariant="playback_bar"
          />
        </>
      ) : null}
    </div>
  );

  const stemPopup =
    showPopup && popupPos
      ? createPortal(
          <div
            ref={popupRef}
            data-state="open"
            data-stem-popup="true"
            className={popupSurfaceClassName}
            style={{ left: popupPos.left, bottom: popupPos.bottom }}
          >
            {collapsedMode ? tightPopupRows : fourStemPopupRows}
          </div>,
          document.body,
        )
      : null;

  if (collapsedMode) {
    return (
      <div ref={tightAnchorRef} className="relative shrink-0">
        <Tooltip label={triggerLabel}>
          <button
            ref={triggerRef}
            type="button"
            onClick={() => {
              if (stemsAvailable) {
                setIsExpanded((open) => !open);
              }
            }}
            disabled={!stemsAvailable}
            aria-label={triggerLabel}
            aria-pressed={stemsAvailable ? isExpanded : undefined}
            data-playback-action="stem-mixer"
            data-active={isExpanded && stemsAvailable ? "true" : undefined}
            className={`motion-icon-button playback-bar-action-button ${
              stemsAvailable
                ? isExpanded
                  ? "text-[var(--color-control-primary)]"
                  : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
                : "text-[var(--color-text-dimmer)]"
            }`}
          >
            <SlidersHorizontal size={18} />
          </button>
        </Tooltip>
        {stemPopup}
      </div>
    );
  }

  return (
    <div
      className={`flex items-center ${
        density === "relaxed" ? "gap-5" : "gap-3"
      }`}
    >
      <StemSlider
        icon={Mic2}
        label={t("stems.vocals")}
        value={stemVolumes.vocals}
        onChange={(v) => handleStemChange("vocals", v)}
        onIconClick={stemsAvailable ? handleVocalsMuteToggle : undefined}
        disabled={!stemsAvailable}
        sliderWidthClass={inlineSliderWidthClass}
        iconButtonVariant="playback_bar"
        playbackActionName="vocals-mute"
      />

      <div className="flex items-center gap-2">
        <StemSlider
          icon={Music}
          label={t("stems.accompaniment")}
          value={accompDragValue ?? accompValue}
          onChange={handleAccompChange}
          onDragStart={handleAccompDragStart}
          onDragEnd={handleAccompDragEnd}
          onIconClick={stemsAvailable ? handleAccompMuteToggle : undefined}
          disabled={!stemsAvailable}
          sliderWidthClass={inlineSliderWidthClass}
          iconButtonVariant="playback_bar"
          playbackActionName="accompaniment-mute"
          muteButtonRef={accompMuteButtonRef}
        />
        {canExpandFourStem ? (
          <Tooltip label={triggerLabel}>
            <button
              ref={triggerRef}
              type="button"
              onClick={() => setIsExpanded(!isExpanded)}
              aria-label={triggerLabel}
              aria-pressed={isExpanded}
              data-playback-action="stem-mixer"
              data-active={isExpanded ? "true" : undefined}
              className="motion-icon-button flex h-4 w-4 items-center justify-center rounded-full text-[var(--color-text-dimmer)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-control-primary)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]"
            >
              <ChevronDown
                size={12}
                className={`transition-transform ${isExpanded ? "rotate-180" : ""}`}
              />
            </button>
          </Tooltip>
        ) : null}
        {stemPopup}
      </div>
    </div>
  );
}

type StemIconButtonVariant = "playback_bar" | "panel";

interface StemSliderProps {
  icon: LucideIcon;
  iconButtonVariant?: StemIconButtonVariant; // default "panel"
  playbackActionName?: "vocals-mute" | "accompaniment-mute";
  panelIconSize?: 14 | 16 | 18; // default 18 (matches inline playback-bar icons)
  label: string;
  value: number;
  onChange: (value: number) => void;
  onDragStart?: () => void;
  onDragEnd?: () => void;
  onIconClick?: () => void;
  disabled?: boolean;
  sliderWidthClass?: string;
  /** Optional ref to the mute icon button (popup positioning anchor). */
  muteButtonRef?: React.RefObject<HTMLButtonElement | null>;
}

export function StemSlider({
  icon: Icon,
  iconButtonVariant = "panel",
  playbackActionName,
  panelIconSize = 18,
  label,
  value,
  onChange,
  onDragStart,
  onDragEnd,
  onIconClick,
  disabled = false,
  sliderWidthClass = "w-16 mr-[14px]",
  muteButtonRef,
}: StemSliderProps) {
  const { t } = useTranslation();
  const muteLabel =
    value === 0
      ? t("stems.unmute", { stem: label })
      : t("stems.mute", { stem: label });

  const isPlaybackBar = iconButtonVariant === "playback_bar";
  const iconSize = isPlaybackBar ? 18 : panelIconSize;
  const isOperational = !disabled && onIconClick != null;
  const isMuted = value === 0;

  /*
   * Muted = dim icon only (same gray as disabled/unselected). Do not set
   * data-active — that class paints a persistent rounded selected chrome
   * which reads as "pressed", not "muted".
   */
  const muteIconClass = isOperational
    ? isMuted
      ? "text-[var(--color-text-dimmer)]"
      : "text-[var(--color-control-primary)] hover:text-[var(--color-text)]"
    : "text-[var(--color-text-dimmer)]";

  if (isPlaybackBar) {
    return (
      <div className="flex items-center gap-2">
        <Tooltip label={onIconClick ? muteLabel : label}>
          <button
            ref={muteButtonRef}
            type="button"
            onClick={onIconClick}
            disabled={disabled || !onIconClick}
            aria-label={onIconClick ? muteLabel : label}
            aria-pressed={isOperational ? isMuted : undefined}
            data-playback-action={playbackActionName}
            className={`motion-icon-button playback-bar-action-button ${muteIconClass}`}
          >
            <Icon size={iconSize} />
          </button>
        </Tooltip>
        <AudioLevelSlider
          label={label}
          value={value}
          onChange={onChange}
          onDragStart={onDragStart}
          onDragEnd={onDragEnd}
          disabled={disabled}
          widthClass={sliderWidthClass}
          ariaLabel={label}
        />
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2">
      <Tooltip label={onIconClick ? muteLabel : label}>
        <button
          ref={muteButtonRef}
          type="button"
          onClick={onIconClick}
          disabled={disabled || !onIconClick}
          aria-pressed={isOperational ? isMuted : undefined}
          className={`motion-icon-button panel-stem-action-button ${
            isOperational
              ? isMuted
                ? "text-[var(--color-text-dimmer)] hover:bg-[var(--color-ghost-hover)]"
                : "text-[var(--color-control-primary)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)]"
              : "text-[var(--color-text-dimmer)]"
          }`}
          aria-label={onIconClick ? muteLabel : label}
        >
          <Icon size={iconSize} />
        </button>
      </Tooltip>
      <AudioLevelSlider
        label={label}
        value={value}
        onChange={onChange}
        onDragStart={onDragStart}
        onDragEnd={onDragEnd}
        disabled={disabled}
        widthClass={sliderWidthClass}
        ariaLabel={label}
      />
    </div>
  );
}
