import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

/** UI boundary; browser component tests supply an explicit in-memory implementation. */
export interface DesktopBackend {
  available: boolean;
  invoke: typeof invoke;
  listen: typeof listen;
  openUrl: typeof openUrl;
  autostart: { enable: typeof enable; disable: typeof disable; isEnabled: typeof isEnabled };
}
export const desktopBackend: DesktopBackend = {
  available: isTauri(), invoke, listen, openUrl, autostart: { enable, disable, isEnabled },
};
