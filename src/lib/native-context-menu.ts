import { LogicalPosition } from "@tauri-apps/api/dpi";
import {
  Menu,
  MenuItem,
  PredefinedMenuItem,
  Submenu,
} from "@tauri-apps/api/menu";
import type { ContextMenuItem } from "@/components/Library/ContextMenu";

/**
 * Build a Tauri native `Menu` from a `ContextMenuItem[]` tree and show it as a
 * popup at the given screen position.  Returns once the user has selected an
 * item (or dismissed the menu).
 */
export async function showNativeContextMenu(
  items: ContextMenuItem[],
  x: number,
  y: number,
): Promise<void> {
  const menuItems = await buildMenuItems(items);
  const menu = await Menu.new({ items: menuItems });
  await menu.popup(new LogicalPosition(Math.round(x), Math.round(y)));
}

async function buildMenuItems(
  items: ContextMenuItem[],
): Promise<(MenuItem | Submenu | PredefinedMenuItem)[]> {
  const result: (MenuItem | Submenu | PredefinedMenuItem)[] = [];

  for (const item of items) {
    if (item.children && item.children.length > 0) {
      const childItems = await buildMenuItems(item.children);
      result.push(
        await Submenu.new({
          text: item.label,
          items: childItems,
        }),
      );
    } else {
      const mi = await MenuItem.new({
        text: item.label,
        action: () => item.onClick?.(),
      });
      if (item.indicator === "checked") {
      }
      result.push(mi);
    }
  }

  return result;
}
