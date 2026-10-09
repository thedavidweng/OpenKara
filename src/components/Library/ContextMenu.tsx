import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronRight } from "lucide-react";
import { getContextMenuPosition } from "./context-menu-position";
import { isInSafetyZone } from "./submenu-safety-zone";
import type { Point } from "./submenu-safety-zone";

export interface ContextMenuItem {
  label: string;
  children?: ContextMenuItem[];
  onClick?: () => void;
  indicator?: "checked" | "mixed" | null;
  disabled?: boolean;
}

interface ContextMenuProps {
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
  triggerRef?: React.RefObject<HTMLElement | null>;
}

export function ContextMenu({
  x,
  y,
  items,
  onClose,
  triggerRef,
}: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: x, top: y });
  const [focusedIndex, setFocusedIndex] = useState(0);
  const [submenuItem, setSubmenuItem] = useState<{
    children: ContextMenuItem[];
    parentRect: DOMRect;
  } | null>(null);
  const [submenuOpenIndex, setSubmenuOpenIndex] = useState<number | null>(null);
  const isOverSubmenu = useRef(false);
  const hideTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mousePosRef = useRef<Point>({ x: 0, y: 0 });
  const parentItemRectRef = useRef<DOMRect | null>(null);
  const submenuRectRef = useRef<DOMRect | null>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const itemCount = items.length;

  const clearHide = () => {
    if (hideTimeout.current) {
      clearTimeout(hideTimeout.current);
      hideTimeout.current = null;
    }
  };

  const scheduleHide = () => {
    clearHide();
    const mouse = mousePosRef.current;
    const parent = parentItemRectRef.current;
    const submenu = submenuRectRef.current;
    if (parent && submenu && isInSafetyZone(mouse, parent, submenu)) {
      return;
    }
    hideTimeout.current = setTimeout(() => {
      if (!isOverSubmenu.current) {
        setSubmenuItem(null);
        setSubmenuOpenIndex(null);
      }
    }, 300);
  };

  useEffect(() => {
    const trigger = triggerRef?.current ?? null;
    if (trigger) {
      trigger.setAttribute("aria-haspopup", "true");
      trigger.setAttribute("aria-expanded", "true");
    }
    return () => {
      if (trigger) {
        trigger.setAttribute("aria-expanded", "false");
      }
    };
  }, [triggerRef]);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as Node;
      const inMenu = menuRef.current?.contains(target);
      const inSubmenu = (target as Element).closest?.("[data-context-submenu]");
      if (!inMenu && !inSubmenu) {
        onClose();
      }
    };
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        triggerRef?.current?.focus();
        onClose();
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleEscape);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [onClose, triggerRef]);

  useEffect(() => {
    return () => clearHide();
  }, []);

  useEffect(() => {
    itemRefs.current[focusedIndex]?.focus();
  }, [focusedIndex]);

  useLayoutEffect(() => {
    const updatePosition = () => {
      const menu = menuRef.current;
      if (!menu) {
        return;
      }

      const rect = menu.getBoundingClientRect();
      setPosition(
        getContextMenuPosition({
          x,
          y,
          menuWidth: rect.width,
          menuHeight: rect.height,
          viewportWidth: window.innerWidth,
          viewportHeight: window.innerHeight,
        }),
      );
    };

    updatePosition();
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);

    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [x, y, items, itemCount]);

  if (typeof document === "undefined" || !document.body) {
    return null;
  }

  const handleMenuKeyDown = (e: React.KeyboardEvent) => {
    if (itemCount === 0) return;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setFocusedIndex((prev) => (prev + 1) % itemCount);
        break;
      case "ArrowUp":
        e.preventDefault();
        setFocusedIndex((prev) => (prev - 1 + itemCount) % itemCount);
        break;
      case "Home":
        e.preventDefault();
        setFocusedIndex(0);
        break;
      case "End":
        e.preventDefault();
        setFocusedIndex(itemCount - 1);
        break;
      case "ArrowRight":
      case "Enter":
      case " ": {
        const item = items[focusedIndex];
        if (item.children) {
          e.preventDefault();
          const btn = itemRefs.current[focusedIndex];
          if (btn) {
            clearHide();
            parentItemRectRef.current = btn.getBoundingClientRect();
            setSubmenuOpenIndex(focusedIndex);
            setSubmenuItem({
              children: item.children,
              parentRect: btn.getBoundingClientRect(),
            });
          }
        } else if (e.key !== "ArrowRight") {
          e.preventDefault();
          item.onClick?.();
          onClose();
        }
        break;
      }
      default:
        if (e.key.length === 1 && /[a-zA-Z0-9]/.test(e.key)) {
          const char = e.key.toLowerCase();
          for (let i = 1; i <= itemCount; i++) {
            const idx = (focusedIndex + i) % itemCount;
            if (items[idx].label.toLowerCase().startsWith(char)) {
              e.preventDefault();
              setFocusedIndex(idx);
              break;
            }
          }
        }
        break;
    }
  };

  return createPortal(
    <>
      <div
        ref={menuRef}
        role="menu"
        aria-orientation="vertical"
        tabIndex={-1}
        className="fixed z-[70] min-w-[140px] rounded-md border border-[var(--color-border)] bg-[var(--color-sidebar)] py-1 shadow-xl"
        style={{ left: position.left, top: position.top }}
        onMouseMove={(e) => {
          mousePosRef.current = { x: e.clientX, y: e.clientY };
        }}
        onKeyDown={handleMenuKeyDown}
      >
        {items.map((item, index) => {
          const isCheckbox = item.indicator !== undefined;
          const role = isCheckbox ? "menuitemcheckbox" : "menuitem";
          const ariaChecked = isCheckbox
            ? item.indicator === "checked"
              ? "true"
              : item.indicator === "mixed"
                ? "mixed"
                : "false"
            : undefined;
          return (
            <button
              key={item.label}
              ref={(el) => {
                itemRefs.current[index] = el;
              }}
              role={role}
              aria-checked={ariaChecked}
              aria-haspopup={item.children ? "true" : undefined}
              aria-expanded={
                item.children
                  ? submenuOpenIndex === index
                    ? "true"
                    : "false"
                  : undefined
              }
              tabIndex={index === focusedIndex ? 0 : -1}
              disabled={item.disabled}
              aria-disabled={item.disabled ? "true" : undefined}
              onClick={() => {
                if (item.disabled) {
                  return;
                }
                if (!item.children) {
                  item.onClick?.();
                  onClose();
                }
              }}
              onMouseEnter={
                item.children
                  ? (e) => {
                      clearHide();
                      setFocusedIndex(index);
                      const btn = e.currentTarget;
                      parentItemRectRef.current = btn.getBoundingClientRect();
                      setSubmenuOpenIndex(index);
                      setSubmenuItem({
                        children: item.children!,
                        parentRect: btn.getBoundingClientRect(),
                      });
                    }
                  : () => {
                      setFocusedIndex(index);
                    }
              }
              onMouseLeave={
                item.children
                  ? () => {
                      scheduleHide();
                    }
                  : undefined
              }
              onFocus={() => setFocusedIndex(index)}
              className="flex w-full items-center px-3 py-1.5 text-left text-[13px] text-[var(--color-text-dim)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text)]"
            >
              <span
                className="mr-2 flex h-4 w-4 shrink-0 items-center justify-center text-[10px] text-[var(--color-accent)]"
                aria-hidden="true"
              >
                {item.indicator === "checked"
                  ? "✓"
                  : item.indicator === "mixed"
                    ? "−"
                    : ""}
              </span>
              <span className="flex-1">{item.label}</span>
              {item.children && <ChevronRight size={14} className="ml-3" />}
            </button>
          );
        })}
      </div>

      {submenuItem && (
        <SubMenu
          parentRect={submenuItem.parentRect}
          items={submenuItem.children}
          onClose={onClose}
          onSubmenuRect={(rect) => {
            submenuRectRef.current = rect;
          }}
          onMouseEnter={() => {
            isOverSubmenu.current = true;
            clearHide();
          }}
          onMouseLeave={() => {
            isOverSubmenu.current = false;
            scheduleHide();
          }}
        />
      )}
    </>,
    document.body,
  );
}

function SubMenu({
  parentRect,
  items,
  onClose,
  onSubmenuRect,
  onMouseEnter,
  onMouseLeave,
}: {
  parentRect: DOMRect;
  items: ContextMenuItem[];
  onClose: () => void;
  onSubmenuRect: (rect: DOMRect) => void;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
}) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [focusedIndex, setFocusedIndex] = useState(0);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const [pos, setPos] = useState({
    left: parentRect.right,
    top: parentRect.top,
  });

  useLayoutEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;

    const rect = menu.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    let left = parentRect.right;
    let top = parentRect.top;

    if (left + rect.width > vw) {
      left = parentRect.left - rect.width;
    }
    if (top + rect.height > vh) {
      top = vh - rect.height;
    }

    setPos({ left, top });
    onSubmenuRect(menu.getBoundingClientRect());
  }, [parentRect, items, onSubmenuRect]);

  useEffect(() => {
    itemRefs.current[0]?.focus();
  }, []);

  useEffect(() => {
    itemRefs.current[focusedIndex]?.focus();
  }, [focusedIndex]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (items.length === 0) return;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setFocusedIndex((prev) => (prev + 1) % items.length);
        break;
      case "ArrowUp":
        e.preventDefault();
        setFocusedIndex((prev) => (prev - 1 + items.length) % items.length);
        break;
      case "Home":
        e.preventDefault();
        setFocusedIndex(0);
        break;
      case "End":
        e.preventDefault();
        setFocusedIndex(items.length - 1);
        break;
      case "ArrowLeft":
      case "Escape":
        e.preventDefault();
        onClose();
        break;
      case "Enter":
      case " ": {
        e.preventDefault();
        const item = items[focusedIndex];
        item.onClick?.();
        onClose();
        break;
      }
      default:
        if (e.key.length === 1 && /[a-zA-Z0-9]/.test(e.key)) {
          const char = e.key.toLowerCase();
          for (let i = 1; i <= items.length; i++) {
            const idx = (focusedIndex + i) % items.length;
            if (items[idx].label.toLowerCase().startsWith(char)) {
              e.preventDefault();
              setFocusedIndex(idx);
              break;
            }
          }
        }
        break;
    }
  };

  return (
    <div
      ref={menuRef}
      role="menu"
      aria-orientation="vertical"
      tabIndex={-1}
      data-context-submenu
      className="fixed z-[71] min-w-[140px] rounded-md border border-[var(--color-border)] bg-[var(--color-sidebar)] py-1 shadow-xl"
      style={{ left: pos.left, top: pos.top }}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      onKeyDown={handleKeyDown}
    >
      {items.map((item, index) => {
        const isCheckbox = item.indicator !== undefined;
        return (
          <button
            key={item.label}
            ref={(el) => {
              itemRefs.current[index] = el;
            }}
            role={isCheckbox ? "menuitemcheckbox" : "menuitem"}
            aria-checked={
              isCheckbox
                ? item.indicator === "checked"
                  ? "true"
                  : item.indicator === "mixed"
                    ? "mixed"
                    : "false"
                : undefined
            }
            tabIndex={index === focusedIndex ? 0 : -1}
            onClick={() => {
              item.onClick?.();
              onClose();
            }}
            onMouseEnter={() => setFocusedIndex(index)}
            onFocus={() => setFocusedIndex(index)}
            className="flex w-full items-center px-3 py-1.5 text-left text-[13px] text-[var(--color-text-dim)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text)]"
          >
            <span
              className="mr-2 flex h-4 w-4 shrink-0 items-center justify-center text-[10px] text-[var(--color-accent)]"
              aria-hidden="true"
            >
              {item.indicator === "checked"
                ? "✓"
                : item.indicator === "mixed"
                  ? "−"
                  : ""}
            </span>
            <span>{item.label}</span>
          </button>
        );
      })}
    </div>
  );
}
