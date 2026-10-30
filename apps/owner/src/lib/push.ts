import { send } from "./api";

/** Why the push switch cannot be turned on here, or how it stands. */
export type PushState = "on" | "off" | "ios" | "unsupported" | "denied" | "unavailable";

/** The browser can show push notifications at all. */
export const pushSupported = () =>
  typeof navigator !== "undefined" &&
  "serviceWorker" in navigator &&
  "PushManager" in window &&
  "Notification" in window;

/** iPhone and iPad (an iPad can present itself as a Mac). */
export const isIos = () =>
  /iPad|iPhone|iPod/.test(navigator.userAgent) ||
  (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);

/** Running as an installed Home Screen app (not in a browser tab). */
export const isInstalled = () =>
  window.matchMedia?.("(display-mode: standalone)").matches === true ||
  (navigator as Navigator & { standalone?: boolean }).standalone === true;

/** iOS delivers web push only to a Home Screen app, so a tab never asks for permission. */
export const needsInstall = () => isIos() && !isInstalled();

/** The browser's key for the server's VAPID public key (base64url). */
function keyBytes(b64url: string): Uint8Array<ArrayBuffer> {
  const pad = "=".repeat((4 - (b64url.length % 4)) % 4);
  const raw = atob((b64url + pad).replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(raw, (c) => c.charCodeAt(0));
}

const registration = () => navigator.serviceWorker.ready;

/** The subscription this browser holds, if any. */
async function current(): Promise<PushSubscription | null> {
  try {
    return await (await registration()).pushManager.getSubscription();
  } catch {
    return null;
  }
}

/** How the push switch stands on this device. `key` is the server's VAPID key (null: no push). */
export async function pushState(key: string | null): Promise<PushState> {
  if (needsInstall()) return "ios";
  if (!pushSupported()) return "unsupported";
  if (Notification.permission === "denied") return "denied";
  if (Notification.permission === "granted" && (await current())) return "on";
  return key ? "off" : "unavailable";
}

/**
 * Turns push on: asks for permission, subscribes with the server's key and hands the subscription
 * to the server. On iOS outside an installed app it never asks.
 */
export async function enablePush(key: string | null): Promise<PushState | "failed"> {
  if (needsInstall()) return "ios";
  if (!pushSupported()) return "unsupported";
  if (!key) return "unavailable";
  if (Notification.permission === "denied") return "denied";
  const permission = await Notification.requestPermission();
  if (permission !== "granted") return "denied";
  try {
    const pushManager = (await registration()).pushManager;
    const sub =
      (await pushManager.getSubscription()) ??
      (await pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: keyBytes(key) }));
    const { endpoint, keys } = sub.toJSON();
    const sent = await send("POST", "/alerts/push", {
      endpoint,
      keys: { p256dh: keys?.p256dh, auth: keys?.auth },
    });
    if (sent.ok) return "on";
    await sub.unsubscribe().catch(() => false);
    return "failed";
  } catch {
    return "failed";
  }
}

/** Turns push off on this device: the server forgets the subscription, then the browser does. */
export async function disablePush(): Promise<boolean> {
  const sub = await current();
  if (!sub) return true;
  const sent = await send("POST", "/alerts/push/remove", { endpoint: sub.endpoint });
  if (!sent.ok) return false;
  await sub.unsubscribe().catch(() => false);
  return true;
}
