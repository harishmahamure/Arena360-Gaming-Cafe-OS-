// The ads gate and the thin client of the Android ads plugin (Story 4.10).
//
// Ads exist only on the Android Hub on the Free tier. The gate is evaluated here, in the shared
// UI, from the Hub's effective tier (`HubSnapshot.tier`, then `entitlement.changed`): off for a
// paid Android Hub, for the Windows Hub and for a Hub whose tier is not known yet. While it is
// off the plugin is never initialised, no request is made and nothing is rendered; on any other
// platform every call below does nothing.

import type { AdRating, AdSlot, Tier } from "@arena/ui/generated";

/** The page-side name of the plugin's JavaScript interface (Android WebView only). */
export const BRIDGE_NAME = "ArenaAds";
/** The window event the plugin delivers its `ad.*` events on (`detail: { event, slot }`). */
export const AD_EVENT = "arena:ad";

export const AD_EVENTS = [
  "ad.requested",
  "ad.loaded",
  "ad.failed",
  "ad.shown",
  "ad.clicked",
  "ad.dismissed",
] as const;
export type AdEventName = (typeof AD_EVENTS)[number];
export type AdEvent = {
  event: AdEventName;
  slot: AdSlot;
  /** The plugin's id for this event (a ULID); the Hub counts an id once. */ id?: string;
};

/** A banner rectangle in CSS pixels of the page. */
export type AdRect = { x: number; y: number; width: number; height: number };

/**
 * What the plugin is told about the branch (Story 4.12): the highest content rating it may ask
 * for, and whether the owner's consent is recorded (without it, requests are non-personalised).
 */
export type AdsConfig = { rating: AdRating; personalised: boolean };

/** Before the branch says otherwise: PG, non-personalised. */
export const DEFAULT_ADS_CONFIG: AdsConfig = { rating: "PG", personalised: false };

/** A stored rating the UI knows; MA, junk and nothing at all are PG, never a looser rating. */
export function adRatingOf(value: unknown): AdRating {
  return value === "G" || value === "T" ? value : "PG";
}

/** What the plugin exposes (`ArenaAds` on `window`). */
export type AdsBridge = {
  init(config: string): string;
  configure(config: string): void;
  bannerPlace(rect: string): void;
  bannerHide(): void;
  bannerDestroy(): void;
  interstitialPreload(): void;
  interstitialShow(): void;
};

/** Ads run only on Android and only when the Hub's effective tier is Free. */
export function adsEnabled(android: boolean, tier: Tier | null | undefined): boolean {
  return android && tier === "free";
}

export function isAdEvent(v: unknown): v is AdEvent {
  if (typeof v !== "object" || v === null) return false;
  const e = v as Record<string, unknown>;
  return (
    AD_EVENTS.includes(e.event as AdEventName) && (e.slot === "banner" || e.slot === "interstitial")
  );
}

export type AdsEnv = {
  android: boolean;
  bridge(): AdsBridge | undefined;
  listen(handler: (e: AdEvent) => void): () => void;
};

export function browserEnv(): AdsEnv {
  return {
    android: typeof navigator !== "undefined" && /Android/.test(navigator.userAgent),
    bridge: () => (globalThis as { ArenaAds?: AdsBridge }).ArenaAds,
    listen(handler) {
      const on = (ev: Event) => {
        const detail = (ev as CustomEvent).detail;
        if (isAdEvent(detail)) handler(detail);
      };
      window.addEventListener(AD_EVENT, on);
      return () => window.removeEventListener(AD_EVENT, on);
    },
  };
}

export type AdsState = {
  /** The gate is on and the plugin is initialised. */
  enabled: boolean;
  /** The plugin is from a debug build (the dev gallery opens only then). */
  debug: boolean;
};

export class AdsClient {
  #env: AdsEnv;
  #bridge: AdsBridge | undefined;
  #stop: (() => void) | null = null;
  #events = new Set<(e: AdEvent) => void>();
  #changed = new Set<(s: AdsState) => void>();
  #config: AdsConfig = DEFAULT_ADS_CONFIG;
  state: AdsState = { enabled: false, debug: false };

  constructor(env: AdsEnv = browserEnv()) {
    this.#env = env;
  }

  /**
   * Re-evaluates the gate for the Hub's effective tier. Turning it on starts the plugin once
   * (off the main thread, on the Kotlin side); turning it off removes any banner and stops
   * listening, and the next `true` starts it again. Returns whether ads are on.
   */
  evaluate(tier: Tier | null | undefined): boolean {
    const bridge = adsEnabled(this.#env.android, tier) ? this.#env.bridge() : undefined;
    if (bridge && !this.#bridge) this.#start(bridge);
    else if (!bridge && this.#bridge) this.#stopAds();
    return this.state.enabled;
  }

  /**
   * Tells the plugin the branch's content rating and consent. Safe before the gate turns on (the
   * plugin then starts with it) and while ads run (the next ad request uses it, no restart).
   */
  configure(next: AdsConfig) {
    const config = { rating: adRatingOf(next.rating), personalised: next.personalised === true };
    if (
      config.rating === this.#config.rating &&
      config.personalised === this.#config.personalised
    ) {
      return;
    }
    const previous = this.#config;
    this.#config = config;
    try {
      this.#bridge?.configure(JSON.stringify(config));
    } catch {
      // The plugin did not take it (or is gone with the page): forget it, so the next call sends it again.
      this.#config = previous;
    }
  }

  #start(bridge: AdsBridge) {
    let answer: string;
    try {
      answer = bridge.init(JSON.stringify(this.#config));
    } catch {
      // The plugin did not start: stay off, and try again at the next evaluation.
      return;
    }
    let debug = false;
    try {
      debug = (JSON.parse(answer) as { debug?: unknown }).debug === true;
    } catch {
      // A plugin that answers nothing readable is treated as a release build.
    }
    this.#bridge = bridge;
    this.#stop = this.#env.listen((e) => this.#events.forEach((f) => f(e)));
    this.#set({ enabled: true, debug });
  }

  #stopAds() {
    try {
      this.#bridge?.bannerDestroy();
    } catch {
      // The plugin is gone with the page; nothing to clean up.
    }
    this.#stop?.();
    this.#stop = null;
    this.#bridge = undefined;
    this.#set({ enabled: false, debug: false });
  }

  #set(next: AdsState) {
    this.state = next;
    this.#changed.forEach((f) => f(next));
  }

  onEvent(f: (e: AdEvent) => void): () => void {
    this.#events.add(f);
    return () => this.#events.delete(f);
  }

  onChange(f: (s: AdsState) => void): () => void {
    this.#changed.add(f);
    return () => this.#changed.delete(f);
  }

  readonly banner = {
    place: (rect: AdRect) => this.#bridge?.bannerPlace(JSON.stringify(rect)),
    hide: () => this.#bridge?.bannerHide(),
    destroy: () => this.#bridge?.bannerDestroy(),
  };

  readonly interstitial = {
    preload: () => this.#bridge?.interstitialPreload(),
    show: () => this.#bridge?.interstitialShow(),
  };
}

/** The Hub's one client; every screen reads `ads.state`. */
export const ads = new AdsClient();
