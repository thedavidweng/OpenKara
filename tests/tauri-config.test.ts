import { describe, expect, test } from "vitest";
import baseConfig from "../src-tauri/tauri.conf.json";
import linuxConfig from "../src-tauri/tauri.linux.conf.json";
import macosConfig from "../src-tauri/tauri.macos.conf.json";
import windowsConfig from "../src-tauri/tauri.windows.conf.json";

type WindowConfig = {
  label?: string;
  create?: boolean;
  width?: number;
  minWidth?: number;
  height?: number;
  resizable?: boolean;
};

type TauriConfig = {
  app?: {
    windows?: WindowConfig[];
  };
  bundle?: {
    macOS?: {
      entitlements?: string;
    };
  };
};

const configs: Array<[string, TauriConfig]> = [
  ["base", baseConfig],
  ["linux", linuxConfig],
  ["macos", macosConfig],
  ["windows", windowsConfig],
];

function mainWindow(config: TauriConfig) {
  return config.app?.windows?.find((window) => {
    return (window.label ?? "main") === "main";
  });
}

describe("Tauri window config", () => {
  test.each(configs)("%s config defines a startup main window", (_, config) => {
    const window = mainWindow(config);

    expect(window).toBeDefined();
    expect(window?.create ?? true).toBe(true);
    expect(window?.width).toBe(1280);
    expect(window?.minWidth).toBe(680);
    expect(window?.height).toBe(800);
    expect(window?.resizable).toBe(true);
  });
});
