// When a full-screen ad may appear (Story 4.13). Exactly one trigger: staff unlock the idle lock
// with their PIN. The plugin preloads a standard interstitial while the lock screen is up; on
// unlock it is shown only if it is loaded, the last one was 15 minutes or more ago, and no
// ad-exclusion surface was open when the lock started. Otherwise the Hub returns at once: nothing
// here ever waits for an ad, and a skipped ad is not delayed to a later moment.
//
// The ad itself (its close control and timing) is Google's; the page only learns it ended from
// `ad.dismissed` or `ad.failed`, or from coming back to the foreground.

import type { AdEvent } from "./ads";

/** At most one interstitial per this long. */
export const INTERSTITIAL_GAP_MS = 15 * 60_000;
/** The longest the page waits for the plugin to say the ad ended before it stops waiting. */
export const INTERSTITIAL_MAX_MS = 2 * 60_000;

/** The part of the ads client the policy drives. */
export type InterstitialAds = {
  readonly state: { readonly enabled: boolean };
  onEvent(f: (e: AdEvent) => void): () => void;
  readonly interstitial: { preload(): void; show(): void };
};

export type InterstitialClock = {
  now(): number;
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(id: unknown): void;
};

const REAL_CLOCK: InterstitialClock = {
  now: () => Date.now(),
  setTimeout: (fn, ms) => globalThis.setTimeout(fn, ms),
  clearTimeout: (id) => globalThis.clearTimeout(id as number),
};

export class InterstitialPolicy {
  #ads: InterstitialAds;
  #clock: InterstitialClock;
  #stop: () => void;
  #listeners = new Set<(showing: boolean) => void>();
  /** The idle lock is up. */
  #locked = false;
  /** An ad-exclusion surface was open when the lock started. */
  #excluded = false;
  #loaded = false;
  #lastShownAt: number | null = null;
  #showing = false;
  #timer: unknown = null;

  constructor(ads: InterstitialAds, clock: InterstitialClock = REAL_CLOCK) {
    this.#ads = ads;
    this.#clock = clock;
    this.#stop = ads.onEvent((e) => this.#onAdEvent(e));
  }

  /** An interstitial is on screen: the idle countdown waits until it ends. */
  get showing(): boolean {
    return this.#showing;
  }

  onShowing(f: (showing: boolean) => void): () => void {
    this.#listeners.add(f);
    return () => this.#listeners.delete(f);
  }

  /** The 15-minute spacing has passed (or no interstitial has shown yet). */
  #spaced(): boolean {
    const last = this.#lastShownAt;
    if (last === null) return true;
    const now = this.#clock.now();
    // A wall clock that went backwards cannot hold an ad back for hours.
    return now < last || now - last >= INTERSTITIAL_GAP_MS;
  }

  /**
   * The lock screen is showing. `open` lists the ad-exclusion surfaces open at this moment; with
   * any, or inside the 15 minutes, no ad is requested for this unlock.
   */
  lock(open: readonly string[]) {
    this.#locked = true;
    this.#excluded = open.length > 0;
    if (this.#ads.state.enabled && !this.#excluded && this.#spaced()) {
      this.#ads.interstitial.preload();
    }
  }

  /**
   * Staff unlocked with a PIN and the Hub is back on screen. Shows the interstitial when the rules
   * allow and returns `true` (the page then waits for [onShowing] to say it ended); `false` means
   * the Hub carries on at once.
   */
  unlock(): boolean {
    const excluded = this.#excluded;
    const wasLocked = this.#locked;
    this.#locked = false;
    this.#excluded = false;
    if (!wasLocked || excluded || !this.#loaded || !this.#ads.state.enabled || !this.#spaced()) {
      return false;
    }
    if (this.#showing) return false;
    this.#loaded = false;
    this.#set(true);
    this.#timer = this.#clock.setTimeout(() => this.ended(), INTERSTITIAL_MAX_MS);
    try {
      this.#ads.interstitial.show();
    } catch {
      this.ended();
      return false;
    }
    return true;
  }

  /** Lock ended without a PIN unlock here (signed out, or unlocked in another window). */
  cancel() {
    this.#locked = false;
    this.#excluded = false;
  }

  /** The ad is over (dismissed, failed, left through, or the page is back in the foreground). */
  ended() {
    if (!this.#showing) return;
    if (this.#timer !== null) this.#clock.clearTimeout(this.#timer);
    this.#timer = null;
    this.#set(false);
  }

  dispose() {
    this.#stop();
    this.ended();
    this.#listeners.clear();
  }

  #onAdEvent(e: AdEvent) {
    if (e.slot !== "interstitial") return;
    switch (e.event) {
      case "ad.loaded":
        this.#loaded = true;
        break;
      case "ad.shown":
        this.#lastShownAt = this.#clock.now();
        break;
      case "ad.failed":
        this.#loaded = false;
        this.ended();
        break;
      case "ad.dismissed":
        this.ended();
        break;
      default:
        break;
    }
  }

  #set(next: boolean) {
    this.#showing = next;
    this.#listeners.forEach((f) => f(next));
  }
}
