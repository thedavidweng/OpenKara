import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

/**
 * Posts a native notification, but only while the window is in the background.
 *
 * Separation is minutes of inference and users routinely switch away while it
 * runs. The in-app notification store already covers the focused case, so
 * notifying there too would surface every completion twice.
 *
 * Permission is requested on demand rather than at launch: on macOS the first
 * `requestPermission` raises the system prompt, and asking before the user has
 * ever run a separation asks for a capability the app has not yet earned.
 *
 * This never rejects. Every caller is an event handler, so a rejected promise
 * would become an unhandled rejection rather than anything the user can act on,
 * and the permission APIs genuinely fail on some setups (no notification daemon
 * on Linux).
 */
export async function notifyWhenUnfocused(
  title: string,
  body: string,
): Promise<void> {
  if (document.hasFocus()) {
    return;
  }

  try {
    const granted =
      (await isPermissionGranted()) ||
      (await requestPermission()) === "granted";
    if (granted) {
      sendNotification({ title, body });
    }
  } catch (error) {
    console.warn("[notifications] could not post a native notification", error);
  }
}
