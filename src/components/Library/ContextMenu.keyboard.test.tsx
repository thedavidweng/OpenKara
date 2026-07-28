// @vitest-environment jsdom

import { createRoot } from "react-dom/client";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { ContextMenu } from "./ContextMenu";
import type { ContextMenuItem } from "./ContextMenu";

function renderMenu(
  items: ContextMenuItem[],
  opts: {
    onClose?: () => void;
    triggerRef?: React.RefObject<HTMLElement | null>;
  } = {},
) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  const onClose = opts.onClose ?? vi.fn();
  act(() => {
    root.render(
      <ContextMenu
        x={10}
        y={10}
        items={items}
        onClose={onClose}
        triggerRef={opts.triggerRef}
      />,
    );
  });
  return { container, root, onClose };
}

describe("ContextMenu keyboard navigation and ARIA", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    document.body.innerHTML = "";
  });

  afterEach(() => {
    if (root) act(() => root.unmount());
    if (container) container.remove();
  });

  test("renders role=menu and role=menuitem for each item", () => {
    const items: ContextMenuItem[] = [
      { label: "Play", onClick: () => {} },
      { label: "Delete", onClick: () => {} },
    ];
    ({ container, root } = renderMenu(items));

    const menu = document.querySelector('[role="menu"]');
    expect(menu).not.toBeNull();
    expect(menu?.getAttribute("aria-orientation")).toBe("vertical");

    const menuItems = document.querySelectorAll('[role="menuitem"]');
    expect(menuItems).toHaveLength(2);
  });

  test("renders role=menuitemcheckbox with aria-checked for indicator items", () => {
    const items: ContextMenuItem[] = [
      { label: "Toggle", onClick: () => {}, indicator: "checked" },
      { label: "Partial", onClick: () => {}, indicator: "mixed" },
      { label: "Off", onClick: () => {}, indicator: null },
    ];
    ({ container, root } = renderMenu(items));

    const checkboxes = document.querySelectorAll('[role="menuitemcheckbox"]');
    expect(checkboxes).toHaveLength(3);
    expect(checkboxes[0].getAttribute("aria-checked")).toBe("true");
    expect(checkboxes[1].getAttribute("aria-checked")).toBe("mixed");
    expect(checkboxes[2].getAttribute("aria-checked")).toBe("false");
  });

  test("ArrowDown moves focus to the next item with wrap-around", () => {
    const items: ContextMenuItem[] = [
      { label: "Alpha", onClick: () => {} },
      { label: "Beta", onClick: () => {} },
      { label: "Gamma", onClick: () => {} },
    ];
    ({ container, root } = renderMenu(items));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    const buttons = menu.querySelectorAll("button");

    expect(document.activeElement).toBe(buttons[0]);

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(buttons[1]);

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(buttons[2]);

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(buttons[0]);
  });

  test("ArrowUp moves focus to the previous item with wrap-around", () => {
    const items: ContextMenuItem[] = [
      { label: "Alpha", onClick: () => {} },
      { label: "Beta", onClick: () => {} },
    ];
    ({ container, root } = renderMenu(items));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    const buttons = menu.querySelectorAll("button");

    expect(document.activeElement).toBe(buttons[0]);

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(buttons[1]);
  });

  test("Home and End move focus to first and last item", () => {
    const items: ContextMenuItem[] = [
      { label: "Alpha", onClick: () => {} },
      { label: "Beta", onClick: () => {} },
      { label: "Gamma", onClick: () => {} },
    ];
    ({ container, root } = renderMenu(items));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    const buttons = menu.querySelectorAll("button");

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "End", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(buttons[2]);

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Home", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(buttons[0]);
  });

  test("Enter activates the focused item and closes the menu", () => {
    const onClick = vi.fn();
    const onClose = vi.fn();
    const items: ContextMenuItem[] = [{ label: "Play", onClick }];
    ({ container, root } = renderMenu(items, { onClose }));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });

    expect(onClick).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  test("Space activates the focused item and closes the menu", () => {
    const onClick = vi.fn();
    const onClose = vi.fn();
    const items: ContextMenuItem[] = [{ label: "Play", onClick }];
    ({ container, root } = renderMenu(items, { onClose }));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: " ", bubbles: true }),
      );
    });

    expect(onClick).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  test("Escape closes the menu and returns focus to the trigger", () => {
    const onClose = vi.fn();
    const trigger = document.createElement("button");
    document.body.appendChild(trigger);
    const triggerRef = { current: trigger };
    const items: ContextMenuItem[] = [{ label: "Play", onClick: () => {} }];
    ({ container, root } = renderMenu(items, { onClose, triggerRef }));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    });

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(trigger);

    trigger.remove();
  });

  test("triggerRef sets aria-haspopup and aria-expanded on the trigger", () => {
    const trigger = document.createElement("button");
    document.body.appendChild(trigger);
    const triggerRef = { current: trigger };
    const items: ContextMenuItem[] = [{ label: "Play", onClick: () => {} }];
    ({ container, root } = renderMenu(items, { triggerRef }));

    expect(trigger.getAttribute("aria-haspopup")).toBe("true");
    expect(trigger.getAttribute("aria-expanded")).toBe("true");

    act(() => root.unmount());
    expect(trigger.getAttribute("aria-expanded")).toBe("false");

    trigger.remove();
  });

  test("ArrowRight opens a submenu for items with children", () => {
    const items: ContextMenuItem[] = [
      {
        label: "Sort by",
        children: [{ label: "Title", onClick: () => {} }],
      },
    ];
    ({ container, root } = renderMenu(items));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    const parentButton = menu.querySelector("button") as HTMLButtonElement;
    expect(parentButton.getAttribute("aria-haspopup")).toBe("true");
    expect(parentButton.getAttribute("aria-expanded")).toBe("false");

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });

    expect(parentButton.getAttribute("aria-expanded")).toBe("true");
    const submenu = document.querySelectorAll('[role="menu"]');
    expect(submenu).toHaveLength(2);
  });

  test("type-ahead moves focus to the first item matching the typed letter", () => {
    const items: ContextMenuItem[] = [
      { label: "Alpha", onClick: () => {} },
      { label: "Beta", onClick: () => {} },
      { label: "Gamma", onClick: () => {} },
    ];
    ({ container, root } = renderMenu(items));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    const buttons = menu.querySelectorAll("button");

    expect(document.activeElement).toBe(buttons[0]);

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "g", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(buttons[2]);

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "b", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(buttons[1]);
  });

  test("click on a leaf item activates it and closes the menu", () => {
    const onClick = vi.fn();
    const onClose = vi.fn();
    const items: ContextMenuItem[] = [{ label: "Play", onClick }];
    ({ container, root } = renderMenu(items, { onClose }));

    const button = document.querySelector(
      '[role="menuitem"]',
    ) as HTMLButtonElement;

    act(() => {
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(onClick).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  test("mouse enter on a submenu parent opens the submenu", () => {
    const items: ContextMenuItem[] = [
      {
        label: "Sort by",
        children: [{ label: "Title", onClick: () => {} }],
      },
    ];
    ({ container, root } = renderMenu(items));

    const parentButton = document.querySelector(
      '[role="menuitem"]',
    ) as HTMLButtonElement;

    act(() => {
      parentButton.dispatchEvent(
        new MouseEvent("mouseover", {
          bubbles: true,
          relatedTarget: document.body,
        }),
      );
    });

    expect(parentButton.getAttribute("aria-expanded")).toBe("true");
    const submenus = document.querySelectorAll('[role="menu"]');
    expect(submenus).toHaveLength(2);
  });

  test("mouse enter on a leaf item sets focus index", () => {
    const items: ContextMenuItem[] = [
      { label: "Alpha", onClick: () => {} },
      { label: "Beta", onClick: () => {} },
    ];
    ({ container, root } = renderMenu(items));

    const buttons = document.querySelectorAll('[role="menuitem"]');
    const betaButton = buttons[1] as HTMLButtonElement;

    act(() => {
      betaButton.dispatchEvent(
        new MouseEvent("mouseover", {
          bubbles: true,
          relatedTarget: document.body,
        }),
      );
    });

    expect(betaButton.tabIndex).toBe(0);
  });
});

describe("ContextMenu SubMenu keyboard navigation", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    document.body.innerHTML = "";
  });

  afterEach(() => {
    if (root) act(() => root.unmount());
    if (container) container.remove();
  });

  function renderWithSubmenu(onClose?: () => void) {
    const items: ContextMenuItem[] = [
      {
        label: "Sort by",
        children: [
          { label: "Title", onClick: vi.fn() },
          { label: "Artist", onClick: vi.fn() },
        ],
      },
    ];
    const result = renderMenu(items, { onClose });
    ({ container, root } = result);

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });
    return result;
  }

  test("ArrowDown moves focus within the submenu", () => {
    renderWithSubmenu();

    const submenus = document.querySelectorAll('[role="menu"]');
    const submenu = submenus[1] as HTMLElement;
    const subButtons = submenu.querySelectorAll("button");

    expect(document.activeElement).toBe(subButtons[0]);

    act(() => {
      submenu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(subButtons[1]);
  });

  test("ArrowUp moves focus within the submenu with wrap-around", () => {
    renderWithSubmenu();

    const submenus = document.querySelectorAll('[role="menu"]');
    const submenu = submenus[1] as HTMLElement;
    const subButtons = submenu.querySelectorAll("button");

    act(() => {
      submenu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(subButtons[1]);
  });

  test("Enter activates the submenu item and closes the menu", () => {
    const onClose = vi.fn();
    const subOnClick = vi.fn();
    const items: ContextMenuItem[] = [
      {
        label: "Sort by",
        children: [{ label: "Title", onClick: subOnClick }],
      },
    ];
    ({ container, root } = renderMenu(items, { onClose }));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });

    const submenus = document.querySelectorAll('[role="menu"]');
    const submenu = submenus[1] as HTMLElement;

    act(() => {
      submenu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });

    expect(subOnClick).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalled();
  });

  test("Escape closes the submenu and the menu", () => {
    const onClose = vi.fn();
    renderWithSubmenu(onClose);

    const submenus = document.querySelectorAll('[role="menu"]');
    const submenu = submenus[1] as HTMLElement;

    act(() => {
      submenu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    });

    expect(onClose).toHaveBeenCalled();
  });

  test("ArrowLeft closes the submenu", () => {
    const onClose = vi.fn();
    renderWithSubmenu(onClose);

    const submenus = document.querySelectorAll('[role="menu"]');
    const submenu = submenus[1] as HTMLElement;

    act(() => {
      submenu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }),
      );
    });

    expect(onClose).toHaveBeenCalled();
  });

  test("Home and End move focus within the submenu", () => {
    const items: ContextMenuItem[] = [
      {
        label: "Sort by",
        children: [
          { label: "Title", onClick: () => {} },
          { label: "Artist", onClick: () => {} },
          { label: "Album", onClick: () => {} },
        ],
      },
    ];
    ({ container, root } = renderMenu(items));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });

    const submenus = document.querySelectorAll('[role="menu"]');
    const submenu = submenus[1] as HTMLElement;
    const subButtons = submenu.querySelectorAll("button");

    act(() => {
      submenu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "End", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(subButtons[2]);

    act(() => {
      submenu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Home", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(subButtons[0]);
  });

  test("type-ahead moves focus within the submenu", () => {
    const items: ContextMenuItem[] = [
      {
        label: "Sort by",
        children: [
          { label: "Title", onClick: () => {} },
          { label: "Artist", onClick: () => {} },
        ],
      },
    ];
    ({ container, root } = renderMenu(items));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });

    const submenus = document.querySelectorAll('[role="menu"]');
    const submenu = submenus[1] as HTMLElement;
    const subButtons = submenu.querySelectorAll("button");

    act(() => {
      submenu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "a", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(subButtons[1]);
  });

  test("Space activates the submenu item and closes the menu", () => {
    const onClose = vi.fn();
    const subOnClick = vi.fn();
    const items: ContextMenuItem[] = [
      {
        label: "Sort by",
        children: [{ label: "Title", onClick: subOnClick }],
      },
    ];
    ({ container, root } = renderMenu(items, { onClose }));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });

    const submenus = document.querySelectorAll('[role="menu"]');
    const submenu = submenus[1] as HTMLElement;

    act(() => {
      submenu.dispatchEvent(
        new KeyboardEvent("keydown", { key: " ", bubbles: true }),
      );
    });

    expect(subOnClick).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalled();
  });

  test("click on a submenu item activates it and closes the menu", () => {
    const onClose = vi.fn();
    const subOnClick = vi.fn();
    const items: ContextMenuItem[] = [
      {
        label: "Sort by",
        children: [{ label: "Title", onClick: subOnClick }],
      },
    ];
    ({ container, root } = renderMenu(items, { onClose }));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });

    const submenus = document.querySelectorAll('[role="menu"]');
    const submenu = submenus[1] as HTMLElement;
    const subButton = submenu.querySelector("button") as HTMLButtonElement;

    act(() => {
      subButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(subOnClick).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalled();
  });

  test("click on a submenu checkbox item activates it and closes the menu", () => {
    const onClose = vi.fn();
    const subOnClick = vi.fn();
    const items: ContextMenuItem[] = [
      {
        label: "Sort by",
        children: [
          { label: "Title", onClick: subOnClick, indicator: "checked" },
        ],
      },
    ];
    ({ container, root } = renderMenu(items, { onClose }));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });

    const submenus = document.querySelectorAll('[role="menu"]');
    const submenu = submenus[1] as HTMLElement;
    const subCheckbox = submenu.querySelector(
      '[role="menuitemcheckbox"]',
    ) as HTMLButtonElement;

    act(() => {
      subCheckbox.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(subOnClick).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalled();
  });

  test("ArrowRight on a leaf item does nothing", () => {
    const items: ContextMenuItem[] = [
      { label: "Play", onClick: () => {} },
      { label: "Delete", onClick: () => {} },
    ];
    ({ container, root } = renderMenu(items));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    const buttons = menu.querySelectorAll("button");

    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });

    expect(document.querySelectorAll('[role="menu"]')).toHaveLength(1);
    expect(document.activeElement).toBe(buttons[0]);
  });

  test("renders an empty menu with no items when items is empty", () => {
    ({ container, root } = renderMenu([]));

    const menu = document.querySelector('[role="menu"]');
    expect(menu).not.toBeNull();
    expect(menu?.querySelectorAll("button")).toHaveLength(0);
  });

  test("keyboard navigation does nothing when items is empty", () => {
    ({ container, root } = renderMenu([]));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
      );
    });
    expect(document.querySelectorAll('[role="menuitem"]')).toHaveLength(0);
  });

  test("mouse enter on a submenu item sets focus index", () => {
    const items: ContextMenuItem[] = [
      {
        label: "Sort by",
        children: [
          { label: "Title", onClick: () => {} },
          { label: "Artist", onClick: () => {} },
        ],
      },
    ];
    ({ container, root } = renderMenu(items));

    const menu = document.querySelector('[role="menu"]') as HTMLElement;
    act(() => {
      menu.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });

    const submenus = document.querySelectorAll('[role="menu"]');
    const submenu = submenus[1] as HTMLElement;
    const subButtons = submenu.querySelectorAll("button");
    const secondSubButton = subButtons[1] as HTMLButtonElement;

    act(() => {
      secondSubButton.dispatchEvent(
        new MouseEvent("mouseover", {
          bubbles: true,
          relatedTarget: document.body,
        }),
      );
    });

    expect(secondSubButton.tabIndex).toBe(0);
  });
});
