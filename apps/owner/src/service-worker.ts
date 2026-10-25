/// <reference lib="webworker" />
import { version } from "$app/env";
import { assets, immutable } from "$app/manifest";

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
