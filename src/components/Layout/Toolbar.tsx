import { useRef, useState } from "react";
import { PanelLeft, UploadCloud, Settings, Monitor } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ImportButton } from "@/components/Library/ImportButton";
import { Tooltip } from "@/components/Overlay/Tooltip";
import { AirPlayRouteButton } from "@/components/Player/AirPlayRouteButton";
import { MonitorPicker } from "@/components/Player/MonitorPicker";
import {
  createWindowShellStyle,
  getDefaultWindowShellState,
  type WindowShellState,
} from "@/lib/window-shell";
import { APP_SHORTCUTS, getShortcutDisplay } from "@/lib/app-shortcuts";

interface ToolbarProps {
  hideLeadingShellControls?: boolean;
  onImportMenuAction?: () => void | Promise<void>;
  onToggleSidebar: () => void;
  onToggleSettings: () => void;
  previewMode?: boolean;
  shellState?: WindowShellState;
  settingsOpen: boolean;
  sidebarVisible: boolean;
}

export function Toolbar({
  hideLeadingShellControls = false,
  onImportMenuAction,
  onToggleSidebar,
  onToggleSettings,
  previewMode = false,
  shellState,
  settingsOpen,
  sidebarVisible,
}: ToolbarProps) {
  const { t } = useTranslation();
  const [monitorPickerOpen, setMonitorPickerOpen] = useState(false);
  const monitorBtnRef = useRef<HTMLButtonElement>(null);
  const resolvedShellState = shellState ?? getDefaultWindowShellState("mac");
  const macWindowChrome = resolvedShellState.chromeVariant === "mac";
  return (
    <div
      className="relative flex shrink-0 items-center bg-[var(--color-toolbar)] px-4"
      data-window-shell-tier={resolvedShellState.tier}
      style={{
        ...createWindowShellStyle(resolvedShellState),
        height: "var(--window-shell-toolbar-height)",
      }}
    >
      {previewMode && macWindowChrome ? (
        <div
          className="pointer-events-none absolute left-[14px] top-1/2 z-10 flex -translate-y-1/2 gap-1.5"
          data-preview-traffic-lights="true"
          aria-hidden="true"
        >
          <span className="h-3 w-3 rounded-full bg-[#ff5f57] shadow-[inset_0_0_0_1px_rgba(91,0,0,0.2)]" />
          <span className="h-3 w-3 rounded-full bg-[#febc2e] shadow-[inset_0_0_0_1px_rgba(89,51,0,0.2)]" />
          <span className="h-3 w-3 rounded-full bg-[#28c840] shadow-[inset_0_0_0_1px_rgba(0,75,20,0.2)]" />
        </div>
      ) : null}
      {hideLeadingShellControls ? (
        <div
          className="flex items-center gap-3"
          style={
            macWindowChrome
              ? {
                  paddingInlineStart:
                    "var(--window-shell-leading-controls-space)",
                }
              : undefined
          }
        />
      ) : (
        <div
          className="flex items-center gap-3"
          style={
            macWindowChrome
              ? {
                  paddingInlineStart:
                    "var(--window-shell-leading-controls-space)",
                }
              : undefined
          }
        >
          <Tooltip
            label={t("toolbar.toggleSidebar")}
            shortcut={getShortcutDisplay(APP_SHORTCUTS.toggleSidebar)}
          >
            <button
              onClick={onToggleSidebar}
              aria-label={t("toolbar.toggleSidebar")}
              className={`motion-icon-button rounded-xl p-2 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 ${
                sidebarVisible
                  ? "bg-[color-mix(in_srgb,var(--color-hover)_86%,transparent)] text-[var(--color-text)] shadow-[0_10px_24px_rgba(0,0,0,0.16)]"
                  : "text-[var(--color-text-dim)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)]"
              }`}
            >
              <PanelLeft size={16} />
            </button>
          </Tooltip>
          <div className="h-4 w-px bg-[var(--color-border-light)]" />
          <Tooltip
            label={t("toolbar.import")}
            shortcut={getShortcutDisplay(APP_SHORTCUTS.importFiles)}
          >
            <ImportButton
              ariaLabel={t("toolbar.import")}
              onClick={onImportMenuAction}
            >
              <span className="motion-surface flex items-center gap-1.5 rounded-md border border-[var(--color-border-light)] bg-[var(--color-hover)] px-2.5 py-1 text-[12px] font-medium text-[var(--color-text)] hover:border-[color-mix(in_srgb,var(--color-accent)_24%,var(--color-border-light))] hover:bg-[var(--color-active)] hover:text-white">
                <UploadCloud size={14} /> {t("toolbar.import")}
              </span>
            </ImportButton>
          </Tooltip>
        </div>
      )}

      <div className="min-w-0 flex-1 self-stretch px-4" data-tauri-drag-region>
        {/* Keep this strip broad. Packaged mac builds depend on this exact drag
            affordance plus the window drag capability in the default Tauri
            capability set. If either side changes alone, the window stops
            moving. */}
      </div>

      <div className="flex items-center gap-4">
        <Tooltip
          label={t("toolbar.settings")}
          shortcut={getShortcutDisplay(APP_SHORTCUTS.toggleSettings)}
        >
          <button
            onClick={onToggleSettings}
            aria-label={t("toolbar.settings")}
            className={`motion-icon-button rounded-xl p-2 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 ${
              settingsOpen
                ? "bg-[color-mix(in_srgb,var(--color-hover)_86%,transparent)] text-[var(--color-text)] shadow-[0_10px_24px_rgba(0,0,0,0.16)]"
                : "text-[var(--color-text-dim)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)]"
            }`}
          >
            <Settings size={16} />
          </button>
        </Tooltip>
        <AirPlayRouteButton previewMode={previewMode} />
        <div>
          <Tooltip label={t("player.selectMonitor")}>
            <button
              ref={monitorBtnRef}
              onClick={() => setMonitorPickerOpen(!monitorPickerOpen)}
              aria-label={t("player.selectMonitor")}
              className={`motion-icon-button rounded-xl p-2 text-[var(--color-text-dim)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 ${
                monitorPickerOpen
                  ? "bg-[color-mix(in_srgb,var(--color-hover)_86%,transparent)] text-[var(--color-text)] shadow-[0_10px_24px_rgba(0,0,0,0.16)]"
                  : ""
              }`}
            >
              <Monitor size={16} />
            </button>
          </Tooltip>
          {monitorPickerOpen && (
            <MonitorPicker
              onClose={() => setMonitorPickerOpen(false)}
              anchorRef={monitorBtnRef}
            />
          )}
        </div>
      </div>
    </div>
  );
}
