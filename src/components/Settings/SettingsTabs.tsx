import type { KeyboardEvent, ReactNode } from "react";

interface SettingsTab<T extends string> {
  id: T;
  label: ReactNode;
}

interface SettingsTabListProps<T extends string> {
  idPrefix: string;
  label: string;
  tabs: SettingsTab<T>[];
  value: T;
  onChange: (id: T) => void;
  orientation?: "horizontal" | "vertical";
  className?: string;
  tabClassName: (selected: boolean) => string;
}

const settingsPanelId = (idPrefix: string) => `${idPrefix}-panel`;
const settingsTabId = (idPrefix: string, id: string) => `${idPrefix}-tab-${id}`;

/** Automatic-activation tabs with a roving tabindex (WAI-ARIA tabs pattern). */
export function SettingsTabList<T extends string>({
  idPrefix,
  label,
  tabs,
  value,
  onChange,
  orientation = "horizontal",
  className,
  tabClassName,
}: SettingsTabListProps<T>) {
  const [previous, next] =
    orientation === "vertical"
      ? ["ArrowUp", "ArrowDown"]
      : ["ArrowLeft", "ArrowRight"];

  const onKeyDown = (event: KeyboardEvent<HTMLElement>, index: number) => {
    const target =
      event.key === previous
        ? (index - 1 + tabs.length) % tabs.length
        : event.key === next
          ? (index + 1) % tabs.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? tabs.length - 1
              : null;
    if (target === null) return;
    event.preventDefault();
    onChange(tabs[target].id);
    document.getElementById(settingsTabId(idPrefix, tabs[target].id))?.focus();
  };

  return (
    <div
      role="tablist"
      aria-label={label}
      aria-orientation={orientation}
      className={className}
    >
      {tabs.map((tab, index) => (
        <button
          key={tab.id}
          type="button"
          role="tab"
          id={settingsTabId(idPrefix, tab.id)}
          aria-controls={settingsPanelId(idPrefix)}
          aria-selected={value === tab.id}
          tabIndex={value === tab.id ? 0 : -1}
          onClick={() => onChange(tab.id)}
          onKeyDown={(event) => onKeyDown(event, index)}
          className={tabClassName(value === tab.id)}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}

export function SettingsTabPanel({
  idPrefix,
  value,
  className,
  children,
}: {
  idPrefix: string;
  value: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div
      role="tabpanel"
      id={settingsPanelId(idPrefix)}
      aria-labelledby={settingsTabId(idPrefix, value)}
      className={className}
    >
      {children}
    </div>
  );
}
