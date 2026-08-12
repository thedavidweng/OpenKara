// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test } from "vitest";
import { createSettingsHarness } from "@/test-utils/settings-controller";
import {
  SettingsControllerContext,
  useSettings,
} from "./SettingsController.context";

function renderHook(node: React.ReactElement) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => {
    root.render(node);
  });
  return {
    container,
    unmount: () => {
      act(() => root.unmount());
      container.remove();
    },
  };
}

describe("useSettings", () => {
  test("throws outside the provider", () => {
    function Probe() {
      useSettings();
      return null;
    }

    expect(() => renderHook(<Probe />)).toThrow(
      "Settings components must be used within the provider.",
    );
  });

  test("re-renders consumers when the view changes", async () => {
    const harness = createSettingsHarness({
      overrides: { maintenance: { estimateStemsSize: async () => 42 } },
    });

    function Probe() {
      const { view } = useSettings();
      return <span>{view.dialog ?? "none"}</span>;
    }

    const rendered = renderHook(
      <SettingsControllerContext value={harness.controller}>
        <Probe />
      </SettingsControllerContext>,
    );
    expect(rendered.container.textContent).toBe("none");

    await act(async () => {
      await harness.controller.maintenance.openDialog("delete_stems");
    });

    expect(rendered.container.textContent).toBe("delete_stems");
    rendered.unmount();
  });
});
