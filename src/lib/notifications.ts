import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

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
