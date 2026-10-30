/// <reference lib="webworker" />
import { version } from "$app/env";
import { assets, immutable } from "$app/manifest";
import { t } from "@arena/ui/messages";

const sw = self as unknown as ServiceWorkerGlobalScope;
const key = "shell-" + version;
const shell = ["/", ...[...immutable, ...assets].map((f) => "/" + f.path)];

sw.addEventListener("install", (e) => {
  e.waitUntil(
    caches
      .open(key)
      .then((c) => c.addAll(shell))
      .then(() => sw.skipWaiting()),
  );
});

sw.addEventListener("activate", (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((ks) => Promise.all(ks.filter((k) => k !== key).map((k) => caches.delete(k))))
      .then(() => sw.clients.claim()),
  );
});

sw.addEventListener("fetch", (e) => {
  const { request } = e;
  const url = new URL(request.url);
  if (request.method !== "GET" || url.origin !== location.origin || url.pathname.startsWith("/v1/"))
    return;
  e.respondWith(
    caches.open(key).then(async (c) => {
      const nav = request.mode === "navigate";
      const hit = await c.match(nav ? "/" : request, { ignoreVary: true });
      if (nav) return fetch(request).catch(() => hit ?? Response.error());
      return hit && shell.includes(url.pathname)
        ? hit
        : fetch(request).catch(() => hit ?? Response.error());
    }),
  );
});

type PushPayload = { title?: string; body?: string; url?: string; tag?: string };

// An owner alert (Story 8.33): shown as a notification, and any open window reads its list again.
sw.addEventListener("push", (e) => {
  let data: PushPayload = {};
  try {
    data = (e.data?.json() as PushPayload | undefined) ?? {};
  } catch {
    data = { body: e.data?.text() };
  }
  e.waitUntil(
    Promise.all([
      sw.registration.showNotification(data.title || t("app.name"), {
        body: data.body ?? "",
        tag: data.tag,
        data: { url: data.url },
      }),
      sw.clients
        .matchAll({ type: "window", includeUncontrolled: true })
        .then((all) => all.forEach((c) => c.postMessage({ type: "owner-alert" }))),
    ]),
  );
});

// A tap focuses an open window and sends it to the alert's page, else opens one there.
sw.addEventListener("notificationclick", (e) => {
  e.notification.close();
  const wanted = (e.notification.data as { url?: unknown } | null)?.url;
  // Only a path in this app: never another site.
  const asked = typeof wanted === "string" ? new URL(wanted, sw.location.origin) : null;
  const target = (
    asked && asked.origin === sw.location.origin ? asked : new URL("/alerts", sw.location.origin)
  ).href;
  e.waitUntil(
    sw.clients.matchAll({ type: "window", includeUncontrolled: true }).then(async (all) => {
      const open = all.find((c) => new URL(c.url).origin === sw.location.origin);
      if (open) {
        try {
          await open.focus();
          await open.navigate(target);
          return;
        } catch {
          // An uncontrolled window cannot be navigated: open one at the page instead.
        }
      }
      await sw.clients.openWindow(target);
    }),
  );
});
