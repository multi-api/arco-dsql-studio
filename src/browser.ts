import { openUrl } from "@tauri-apps/plugin-opener";

/** Opens `url` in the default browser. Failing to open it is not an error: the page also shows the link and code. */
export async function openInBrowser(url: string): Promise<void> {
  try {
    await openUrl(url);
  } catch {
    // The user can use "Open the browser again" or type the code.
  }
}
