import { useState, useCallback, useMemo, useRef, useEffect } from "react";
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
import type { PlaybackBarDensity } from "./playback-bar-layout";

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

  useEffect(() => {
    throttledSetStemVolumeRef.current.forEach((limiter) => limiter.cancel());
    throttledSetStemVolumeRef.current = new Map();
    return () =>
      throttledSetStemVolumeRef.current.forEach((limiter) => limiter.cancel());
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

  // Track previous non-zero values for mute/unmute toggle
  const prevVocalsRef = useRef(1);
  const prevAccompRef = useRef(1);
  const prevDrumsRef = useRef(1);
  const prevBassRef = useRef(1);
  const prevOtherRef = useRef(1);

  // Click-outside to close popup
  const popupRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!isExpanded) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (
        popupRef.current &&
        !popupRef.current.contains(e.target as Node) &&
        triggerRef.current &&
        !triggerRef.current.contains(e.target as Node)
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

  // Accompaniment display value = max of the three sub-stems
  const accompValue = Math.max(
    stemVolumes.drums,
    stemVolumes.bass,
    stemVolumes.other,
  );

  const handleAccompChange = useCallback(
    (newValue: number) => {
      if (isTwoStem) {
        // In 2-stem mode, set all three sub-stems to the same value;
        // the backend uses max gain as the accompaniment gain.
        handleStemChange("drums", newValue);
        handleStemChange("bass", newValue);
        handleStemChange("other", newValue);
      } else if (accompValue === 0) {
        // All sub-stems are 0; set them all to the new value
        handleStemChange("drums", newValue);
        handleStemChange("bass", newValue);
        handleStemChange("other", newValue);
      } else {
        const ratio = newValue / accompValue;
        handleStemChange("drums", Math.min(1, stemVolumes.drums * ratio));
        handleStemChange("bass", Math.min(1, stemVolumes.bass * ratio));
        handleStemChange("other", Math.min(1, stemVolumes.other * ratio));
      }
    },
    [isTwoStem, accompValue, stemVolumes, handleStemChange],
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
      prevAccompRef.current = accompValue;
      setStemVolume("drums", 0);
      setStemVolume("bass", 0);
      setStemVolume("other", 0);
    } else {
      const prev = prevAccompRef.current;
      setStemVolume("drums", prev);
      setStemVolume("bass", prev);
      setStemVolume("other", prev);
    }
  }, [accompValue, setStemVolume]);

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

  const inlineSliderWidthClass = density === "compact" ? "w-14" : "w-16";
  const collapsedMode = density === "tight";
  const triggerLabel = isExpanded
    ? t("stems.collapseStems")
    : t("stems.expandStems");
  const sharedPanelClassName =
    "stem-popup app-panel-surface absolute bottom-full z-50 mb-3 rounded-lg border border-[color-mix(in_srgb,var(--color-border)_85%,transparent)] bg-[color-mix(in_srgb,var(--color-sidebar)_90%,transparent)] p-4 shadow-[0_20px_40px_rgba(0,0,0,0.32)]";

  const panelContent = (
    <div className="flex flex-col gap-2.5">
      <StemSlider
        icon={Mic2}
        label={t("stems.vocals")}
        value={stemVolumes.vocals}
        onChange={(v) => handleStemChange("vocals", v)}
        onIconClick={stemsAvailable ? handleVocalsMuteToggle : undefined}
        disabled={!stemsAvailable}
        sliderWidthClass="w-16 mr-[14px]"
      />
      <StemSlider
        icon={Music}
        label={t("stems.accompaniment")}
        value={accompValue}
        onChange={handleAccompChange}
        onIconClick={stemsAvailable ? handleAccompMuteToggle : undefined}
        disabled={!stemsAvailable}
        sliderWidthClass="w-16 mr-[14px]"
      />
      {isFourStem && (
        <>
          <div className="h-px bg-[color-mix(in_srgb,var(--color-border)_85%,transparent)]" />
          <StemSlider
            icon={Drum}
            label={t("stems.drums")}
            value={stemVolumes.drums}
            onChange={(v) => handleStemChange("drums", v)}
            onIconClick={handleDrumsMuteToggle}
            disabled={!stemsAvailable}
            sliderWidthClass="w-16 mr-[14px]"
            panelIconSize={16}
          />
          <StemSlider
            icon={Guitar}
            label={t("stems.bass")}
            value={stemVolumes.bass}
            onChange={(v) => handleStemChange("bass", v)}
            onIconClick={handleBassMuteToggle}
            disabled={!stemsAvailable}
            sliderWidthClass="w-16 mr-[14px]"
            panelIconSize={16}
          />
          <StemSlider
            icon={AudioWaveform}
            label={t("stems.other")}
            value={stemVolumes.other}
            onChange={(v) => handleStemChange("other", v)}
            onIconClick={handleOtherMuteToggle}
            disabled={!stemsAvailable}
            sliderWidthClass="w-16 mr-[14px]"
            panelIconSize={16}
          />
        </>
      )}
    </div>
  );

  if (collapsedMode) {
    return (
      <div className="relative shrink-0">
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
                ? // When expanded, show the accent color explicitly (matches
                  // queue/mute buttons) so hover does not strip it.
                  isExpanded
                  ? "text-[var(--color-accent)]"
                  : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
                : "text-[var(--color-text-dimmer)]"
            }`}
          >
            <SlidersHorizontal size={18} />
          </button>
        </Tooltip>

        {stemsAvailable && (
          <div
            ref={popupRef}
            data-state={isExpanded ? "open" : "closed"}
            className={`${sharedPanelClassName} right-0 w-max`}
          >
            {panelContent}
          </div>
        )}
      </div>
    );
  }

  return (
    <div
      className={`flex items-center ${
        density === "relaxed" ? "gap-5" : "gap-3"
      }`}
    >
      {/* Vocals slider */}
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

      {/* Accompaniment group — relative for popup anchor */}
      <div className="relative flex items-center gap-2">
        <StemSlider
          icon={Music}
          label={t("stems.accompaniment")}
          value={accompValue}
          onChange={handleAccompChange}
          onIconClick={stemsAvailable ? handleAccompMuteToggle : undefined}
          disabled={!stemsAvailable}
          sliderWidthClass={inlineSliderWidthClass}
          iconButtonVariant="playback_bar"
          playbackActionName="accompaniment-mute"
        />
        {stemsAvailable && isFourStem && (
          <Tooltip
            label={
              isExpanded ? t("stems.collapseStems") : t("stems.expandStems")
            }
          >
            <button
              ref={triggerRef}
              onClick={() => setIsExpanded(!isExpanded)}
              aria-label={
                isExpanded ? t("stems.collapseStems") : t("stems.expandStems")
              }
              className="motion-icon-button flex h-4 w-4 items-center justify-center rounded-full text-[var(--color-text-dimmer)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-control-primary)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
            >
              <ChevronDown
                size={12}
                className={`transition-transform ${isExpanded ? "rotate-180" : ""}`}
              />
            </button>
          </Tooltip>
        )}

        {/* Popup for individual stem controls — aligned with accompaniment */}
        {stemsAvailable && isFourStem && (
          <div
            ref={popupRef}
            data-state={isExpanded ? "open" : "closed"}
            className={`${sharedPanelClassName} left-0`}
          >
            <div className="flex flex-col gap-2">
              <StemSlider
                icon={Drum}
                label={t("stems.drums")}
                value={stemVolumes.drums}
                onChange={(v) => handleStemChange("drums", v)}
                onIconClick={handleDrumsMuteToggle}
                panelIconSize={16}
              />
              <StemSlider
                icon={Guitar}
                label={t("stems.bass")}
                value={stemVolumes.bass}
                onChange={(v) => handleStemChange("bass", v)}
                onIconClick={handleBassMuteToggle}
                panelIconSize={16}
              />
              <StemSlider
                icon={AudioWaveform}
                label={t("stems.other")}
                value={stemVolumes.other}
                onChange={(v) => handleStemChange("other", v)}
                onIconClick={handleOtherMuteToggle}
                panelIconSize={16}
              />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

type StemIconButtonVariant = "playback_bar" | "panel";

interface StemSliderProps {
  icon: LucideIcon;
  iconButtonVariant?: StemIconButtonVariant; // default "panel"
  playbackActionName?: "vocals-mute" | "accompaniment-mute";
  panelIconSize?: 14 | 16; // default 16 (balanced inside 44px target)
  label: string;
  value: number;
  onChange: (value: number) => void;
  onIconClick?: () => void;
  disabled?: boolean;
  sliderWidthClass?: string;
}

export function StemSlider({
  icon: Icon,
  iconButtonVariant = "panel",
  playbackActionName,
  panelIconSize = 16,
  label,
  value,
  onChange,
  onIconClick,
  disabled = false,
  sliderWidthClass = "w-16 mr-[14px]",
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

  if (isPlaybackBar) {
    return (
      <div className="flex items-center gap-2">
        <Tooltip label={onIconClick ? muteLabel : label}>
          <button
            onClick={onIconClick}
            disabled={disabled || !onIconClick}
            aria-label={onIconClick ? muteLabel : label}
            aria-pressed={isOperational ? isMuted : undefined}
            data-playback-action={playbackActionName}
            data-active={isOperational && isMuted ? "true" : undefined}
            className={`motion-icon-button playback-bar-action-button ${
              isOperational
                ? isMuted
                  ? "text-[var(--color-accent)]"
                  : "text-[var(--color-control-primary)] hover:text-[var(--color-text)]"
                : "text-[var(--color-text-dimmer)]"
            }`}
          >
            <Icon size={iconSize} />
          </button>
        </Tooltip>
        <AudioLevelSlider
          label={label}
          value={value}
          onChange={onChange}
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
          onClick={onIconClick}
          disabled={disabled || !onIconClick}
          aria-pressed={isOperational ? isMuted : undefined}
          data-active={isOperational && isMuted ? "true" : undefined}
          className={`motion-icon-button panel-stem-action-button ${
            isOperational
              ? isMuted
                ? // Muted-but-clickable: use accent so it reads as active, not disabled.
                  "text-[var(--color-accent)] hover:bg-[var(--color-ghost-hover)]"
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
        disabled={disabled}
        widthClass={sliderWidthClass}
        ariaLabel={label}
      />
    </div>
  );
}
