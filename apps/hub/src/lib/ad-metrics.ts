// Counting ads (Story 4.14). The plugin says `ad.requested` each time it asks the SDK for an ad
// and `ad.shown` on the SDK's impression callback, each with an id of its own. The page forwards
// each one, once, to the Hub (`POST /ui/ads/event`), which records one row and puts the day's
// totals in the heartbeat. The post holds a kind, a slot and the event id: nothing about a device,
// a creative or an advertiser.
//
// The Hub counts an id once, so a post that is repeated (a retry after a lost answer) is safe.
// Events only arrive while the ads gate is on (`AdsClient` forwards none otherwise), so a paid or
// Windows Hub posts nothing.

import type { AdEventKind, UiAdEvent } from "@arena/ui/generated";
import type { AdEvent } from "./ads";
import { ulid } from "./ulid";

export const ADS_EVENT_URL = "http://127.0.0.1:7409/ui/ads/event";
/** Waits before each repeat of a post the Hub could not take (it was busy or unreachable). */
export const RETRY_MS = [2_000, 10_000, 30_000, 120_000] as const;
/** The most posts waiting at once; further events are not counted rather than grow without bound. */
export const MAX_PENDING = 500;
/** Ids remembered, so an event delivered twice is posted once. */
const SEEN_MAX = 500;
const TIMEOUT_MS = 5_000;
const ULID = /^[0-7][0-9A-HJKMNP-TV-Za-hjkmnp-tv-z]{25}$/;

/** The part of the ads client the counter listens to. */
export type AdMetricsSource = { onEvent(f: (e: AdEvent) => void): () => void };

export type AdMetricsEnv = {
  /** Resolves with the HTTP status; rejects when the Hub could not be reached. */
  post(body: UiAdEvent): Promise<number>;
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(id: unknown): void;
  /** A fresh event id, for an event that came without one. */
  newId(): string;
};

export function browserMetricsEnv(): AdMetricsEnv {
  return {
    post: async (body) => {
      const res = await fetch(ADS_EVENT_URL, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(TIMEOUT_MS),
      });
      return res.status;
    },
    setTimeout: (fn, ms) => globalThis.setTimeout(fn, ms),
    clearTimeout: (id) => globalThis.clearTimeout(id as number),
    newId: () => ulid(),
  };
}

/** The counted kind of a plugin event, or null for every other event. */
export function metricKind(event: AdEvent["event"]): AdEventKind | null {
  if (event === "ad.requested") return "request";
  if (event === "ad.shown") return "impression";
  return null;
}

/** Whether a post that got this status should be tried again later. */
export function retryable(status: number): boolean {
  return status === 408 || status === 429 || status >= 500;
}

export class AdMetrics {
  #env: AdMetricsEnv;
  #stop: () => void;
  #seen: string[] = [];
  #pending = new Map<string, unknown>();
  #disposed = false;

  constructor(source: AdMetricsSource, env: AdMetricsEnv = browserMetricsEnv()) {
    this.#env = env;
    this.#stop = source.onEvent((e) => this.#onEvent(e));
  }

  /** Posts still waiting for the Hub (sent, or waiting to be sent again). */
  get pending(): number {
    return this.#pending.size;
  }

  dispose() {
    this.#disposed = true;
    this.#stop();
    for (const timer of this.#pending.values()) {
      if (timer !== null) this.#env.clearTimeout(timer);
    }
    this.#pending.clear();
  }

  #onEvent(e: AdEvent) {
    const kind = metricKind(e.event);
    if (!kind) return;
    const id = typeof e.id === "string" && ULID.test(e.id) ? e.id.toUpperCase() : this.#env.newId();
    if (this.#seen.includes(id) || this.#pending.size >= MAX_PENDING) return;
    this.#seen.push(id);
    if (this.#seen.length > SEEN_MAX) this.#seen.shift();
    void this.#send({ kind, slot: e.slot, id }, 0);
  }

  async #send(body: UiAdEvent, attempt: number) {
    this.#pending.set(body.id, null);
    let again = false;
    try {
      again = retryable(await this.#env.post(body));
    } catch {
      again = true;
    }
    if (this.#disposed) return;
    if (!again || attempt >= RETRY_MS.length) {
      this.#pending.delete(body.id);
      return;
    }
    const timer = this.#env.setTimeout(() => void this.#send(body, attempt + 1), RETRY_MS[attempt]);
    this.#pending.set(body.id, timer);
  }
}
