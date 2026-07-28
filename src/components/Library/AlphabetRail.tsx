import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ALPHABET_BUCKETS,
  type AlphabetBucket,
  resolveBucket,
} from "@/lib/alphabet-index";

interface AlphabetRailProps {
  indexByBucket: ReadonlyMap<AlphabetBucket, number>;
  onNavigate: (index: number, bucket: AlphabetBucket) => void;
}

export function AlphabetRail({ indexByBucket, onNavigate }: AlphabetRailProps) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const activePointerIdRef = useRef<number | null>(null);
  const lastNavigatedBucketRef = useRef<AlphabetBucket | null>(null);
  const pointerNavOccurredRef = useRef(false);
  const pointerMovedRef = useRef(false);
  const [rovingBucket, setRovingBucket] = useState<AlphabetBucket | null>(null);
  // Keep the keyboard cursor synchronously available between consecutive
  // key events. React state is intentionally asynchronous, so an immediate
  // End → Enter sequence must not activate the bucket from the previous
  // render.
  const rovingBucketRef = useRef<AlphabetBucket | null>(null);
  const [activeBucket, setActiveBucket] = useState<AlphabetBucket | null>(null);

  const setRovingBucketState = useCallback((bucket: AlphabetBucket | null) => {
    rovingBucketRef.current = bucket;
    setRovingBucket(bucket);
  }, []);

  useEffect(() => {
    const firstMapped =
      ALPHABET_BUCKETS.find((b) => indexByBucket.has(b)) ?? null;
    setRovingBucketState(firstMapped);
    setActiveBucket(null);
    lastNavigatedBucketRef.current = null;
  }, [indexByBucket, setRovingBucketState]);

  const navigateToBucket = useCallback(
    (bucket: AlphabetBucket): boolean => {
      const resolved = resolveBucket(indexByBucket, bucket);
      if (resolved === null) return false;
      if (lastNavigatedBucketRef.current === resolved.bucket) return false;
      lastNavigatedBucketRef.current = resolved.bucket;
      setActiveBucket(resolved.bucket);
      onNavigate(resolved.index, resolved.bucket);
      return true;
    },
    [indexByBucket, onNavigate],
  );

  const bucketFromClientY = useCallback((clientY: number): AlphabetBucket => {
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect) return "#";
    const fraction = rect.height <= 0 ? 0 : (clientY - rect.top) / rect.height;
    const raw = Math.floor(fraction * ALPHABET_BUCKETS.length);
    const index = Math.max(0, Math.min(ALPHABET_BUCKETS.length - 1, raw));
    return ALPHABET_BUCKETS[index];
  }, []);

  const handlePointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      activePointerIdRef.current = event.pointerId;
      pointerMovedRef.current = false;
      event.currentTarget.setPointerCapture(event.pointerId);
      const bucket = bucketFromClientY(event.clientY);
      setRovingBucketState(bucket);
      pointerNavOccurredRef.current = false;
      pointerNavOccurredRef.current = navigateToBucket(bucket);
    },
    [bucketFromClientY, navigateToBucket, setRovingBucketState],
  );

  const handlePointerMove = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (activePointerIdRef.current !== event.pointerId) return;
      pointerMovedRef.current = true;
      const bucket = bucketFromClientY(event.clientY);
      if (bucket !== lastNavigatedBucketRef.current) {
        setRovingBucketState(bucket);
        navigateToBucket(bucket);
      }
    },
    [bucketFromClientY, navigateToBucket, setRovingBucketState],
  );

  const releasePointer = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (activePointerIdRef.current !== event.pointerId) return;
      activePointerIdRef.current = null;
      const pointerMoved = pointerMovedRef.current;
      pointerMovedRef.current = false;
      if (pointerMoved) {
        setActiveBucket(null);
      }
      lastNavigatedBucketRef.current = null;
      if (event.type !== "pointerup") {
        pointerNavOccurredRef.current = false;
      }
      try {
        event.currentTarget.releasePointerCapture(event.pointerId);
      } catch {
        // Pointer may already be released
      }
    },
    [],
  );

  useEffect(() => {
    return () => {
      activePointerIdRef.current = null;
    };
  }, []);

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      const currentRovingBucket = rovingBucketRef.current;
      const currentPos = currentRovingBucket
        ? ALPHABET_BUCKETS.indexOf(currentRovingBucket)
        : 0;
      let nextPos: number | null = null;

      switch (event.key) {
        case "ArrowUp":
        case "ArrowLeft":
          nextPos = Math.max(0, currentPos - 1);
          break;
        case "ArrowDown":
        case "ArrowRight":
          nextPos = Math.min(ALPHABET_BUCKETS.length - 1, currentPos + 1);
          break;
        case "Home":
          nextPos = 0;
          break;
        case "End":
          nextPos = ALPHABET_BUCKETS.length - 1;
          break;
        case "Enter":
        case " ":
          if (currentRovingBucket) {
            event.preventDefault();
            lastNavigatedBucketRef.current = null;
            navigateToBucket(currentRovingBucket);
          }
          return;
        default:
          if (event.key.length === 1 && /[a-zA-Z]/.test(event.key)) {
            const upper = event.key.toUpperCase();
            if (ALPHABET_BUCKETS.includes(upper as AlphabetBucket)) {
              const target = upper as AlphabetBucket;
              setRovingBucketState(target);
              lastNavigatedBucketRef.current = null;
              const button = containerRef.current?.querySelector(
                `[data-bucket="${target}"]`,
              ) as HTMLButtonElement | null;
              button?.focus();
              navigateToBucket(target);
            }
          }
          return;
      }

      if (nextPos !== null) {
        event.preventDefault();
        const nextBucket = ALPHABET_BUCKETS[nextPos];
        setRovingBucketState(nextBucket);
        const button = containerRef.current?.querySelector(
          `[data-bucket="${nextBucket}"]`,
        ) as HTMLButtonElement | null;
        button?.focus();
      }
    },
    [navigateToBucket, setRovingBucketState],
  );

  const labelForBucket = useMemo(
    () =>
      (bucket: AlphabetBucket): string =>
        bucket === "#"
          ? t("sidebar.alphabetRail.other")
          : t("sidebar.alphabetRail.jumpTo", { letter: bucket }),
    [t],
  );

  return (
    <div
      ref={containerRef}
      role="navigation"
      aria-label={t("sidebar.alphabetRail.label")}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={releasePointer}
      onPointerCancel={releasePointer}
      onLostPointerCapture={releasePointer}
      onKeyDown={handleKeyDown}
      className="absolute right-[2px] top-2 bottom-2 z-10 grid grid-rows-27 touch-none select-none"
      style={{ width: "22px" }}
    >
      {ALPHABET_BUCKETS.map((bucket) => {
        const isRoving = rovingBucket === bucket;
        const isActive = activeBucket === bucket;
        return (
          <button
            key={bucket}
            type="button"
            data-bucket={bucket}
            tabIndex={isRoving ? 0 : -1}
            aria-current={isActive ? "true" : undefined}
            aria-label={labelForBucket(bucket)}
            onFocus={() => setRovingBucketState(bucket)}
            onClick={(e) => {
              e.stopPropagation();
              if (pointerNavOccurredRef.current) {
                pointerNavOccurredRef.current = false;
                return;
              }
              setRovingBucketState(bucket);
              lastNavigatedBucketRef.current = null;
              navigateToBucket(bucket);
            }}
            className="flex items-center justify-center text-[9px] leading-none min-h-[14px] text-[var(--color-text-dim)] hover:text-[var(--color-text)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--color-control-primary)] aria-[current=true]:text-[var(--color-control-primary)] aria-[current=true]:font-bold"
          >
            {bucket}
          </button>
        );
      })}
      {activeBucket && (
        <div
          aria-hidden="true"
          className="pointer-events-none absolute right-[26px] flex items-center justify-center rounded-[4px] bg-[var(--color-control-primary)] px-1.5 py-0.5 text-[11px] font-bold text-[var(--color-control-on-primary)]"
          style={{
            top: `${
              (ALPHABET_BUCKETS.indexOf(activeBucket) + 0.5) *
              (100 / ALPHABET_BUCKETS.length)
            }%`,
            transform: "translateY(-50%)",
          }}
        >
          {activeBucket}
        </div>
      )}
    </div>
  );
}
