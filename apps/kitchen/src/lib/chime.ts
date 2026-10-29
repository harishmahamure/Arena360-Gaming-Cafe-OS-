// The new-ticket chime (Story 7.9): one bundled sound, a mute choice kept for this tablet and the
// browser's wait for a first tap before it lets a page make sound. Nothing is fetched from anywhere
// but the Hub that served the page.

/** The bundled chime, a file in `static/sounds` (the build's `paths.base` is `/kitchen`). */
export const CHIME_URL = "/kitchen/sounds/new-ticket.wav"; // i18n-ignore (an address)
/** The browser-storage key of the mute choice. */
export const MUTE_KEY = "arena.kitchen.chime-muted";
/** Tickets arriving within this many milliseconds of the last chime share it. */
export const BURST_MS = 1000;

/** Whether this tablet is muted; unmuted when storage is unavailable or holds nothing. */
export function readMuted(): boolean {
  try {
    return globalThis.localStorage?.getItem(MUTE_KEY) === "1";
  } catch {
    return false;
  }
}

export function storeMuted(muted: boolean) {
  try {
    if (muted) globalThis.localStorage?.setItem(MUTE_KEY, "1");
    else globalThis.localStorage?.removeItem(MUTE_KEY);
  } catch {
    // Storage blocked: the board starts unmuted after a reload.
  }
}

export type Chime = {
  /** Plays the chime once; `false` when the browser refused to play it before a tap. */
  play(): Promise<boolean>;
  /** Whether the browser is holding sound back until the page has been tapped. */
  needsTap(): boolean;
  /** Called from a tap: plays the chime silently so the browser lets the page make sound. */
  unlock(): Promise<boolean>;
};

type AutoplayNavigator = Navigator & { getAutoplayPolicy?: (type: "mediaelement") => string };

const refused = (e: unknown) => (e as { name?: string })?.name === "NotAllowedError";

export function createChime(): Chime {
  let audio: HTMLAudioElement | null = null;
  const element = () => (audio ??= new Audio(CHIME_URL));
  return {
    async play() {
      try {
        const a = element();
        a.volume = 1;
        a.currentTime = 0;
        await a.play();
        return true;
      } catch (e) {
        return !refused(e);
      }
    },
    needsTap() {
      const policy = (globalThis.navigator as AutoplayNavigator | undefined)?.getAutoplayPolicy;
      if (typeof policy === "function") {
        try {
          return policy.call(navigator, "mediaelement") !== "allowed";
        } catch {
          return false;
        }
      }
      return false;
    },
    async unlock() {
      try {
        const a = element();
        a.volume = 0;
        await a.play();
        a.pause();
        a.currentTime = 0;
        a.volume = 1;
        return true;
      } catch (e) {
        return !refused(e);
      }
    },
  };
}
