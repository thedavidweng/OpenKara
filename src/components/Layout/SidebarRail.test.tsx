// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH } from "@/stores/layout-store";
import { SidebarRail } from "./SidebarRail";

describe("SidebarRail", () => {
  test("keeps the rail mounted with a zero-width shell when hidden", () => {
    const markup = renderToStaticMarkup(
      <SidebarRail visible={false} width={300} onResize={() => {}}>
        <div>Sidebar</div>
      </SidebarRail>,
    );

    expect(markup).toContain("overflow-hidden");
    expect(markup).toContain("transition-[width]");
    expect(markup).toContain("w-0");
    expect(markup).toContain("opacity-0");
    expect(markup).toContain("-translate-x-3");
  });

  test("expands the rail and restores the content transform when visible", () => {
    const markup = renderToStaticMarkup(
      <SidebarRail visible width={300} onResize={() => {}}>
        <div>Sidebar</div>
      </SidebarRail>,
    );

    expect(markup).toContain("w-[var(--window-shell-sidebar-width)]");
    expect(markup).toContain("opacity-100");
    expect(markup).toContain("translate-x-0");
    expect(markup).toContain('role="separator"');
  });

  test("disables text selection on the container when visible to prevent selection during drag", () => {
    const markup = renderToStaticMarkup(
      <SidebarRail visible width={300} onResize={() => {}}>
        <div>Sidebar</div>
      </SidebarRail>,
    );

    expect(markup).toContain("select-none");
  });

  test("can keep the preview rail at a fixed width", () => {
    const markup = renderToStaticMarkup(
      <SidebarRail visible width={300} onResize={() => {}} resizable={false}>
        <div>Sidebar</div>
      </SidebarRail>,
    );

    expect(markup).not.toContain('role="separator"');
  });

  test("resizes with standard separator keyboard controls", () => {
    const onResize = vi.fn();
    render(
      <SidebarRail visible width={300} onResize={onResize}>
        <div>Sidebar</div>
      </SidebarRail>,
    );

    const separator = screen.getByRole("separator", {
      name: "Resize sidebar",
    });
    fireEvent.keyDown(separator, { key: "ArrowRight" });
    fireEvent.keyDown(separator, { key: "ArrowLeft" });
    fireEvent.keyDown(separator, { key: "Home" });
    fireEvent.keyDown(separator, { key: "End" });

    expect(onResize).toHaveBeenNthCalledWith(1, 316);
    expect(onResize).toHaveBeenNthCalledWith(2, 284);
    expect(onResize).toHaveBeenNthCalledWith(3, MIN_SIDEBAR_WIDTH);
    expect(onResize).toHaveBeenNthCalledWith(4, MAX_SIDEBAR_WIDTH);
    expect(separator.getAttribute("aria-valuenow")).toBe("300");
  });
});
