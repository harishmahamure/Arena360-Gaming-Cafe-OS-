// The English message catalogue and `t()`. All user-visible text comes from `en.json`.

import en from "./en.json" with { type: "json" };

/** @typedef {keyof typeof en} MessageKey */

/** @type {Record<string, string>} */
export const messages = en;

export const isDev = (() => {
  try {
    // Vite and Astro set `import.meta.env.DEV`; elsewhere (Node, tests) treat as dev.
    const env = /** @type {any} */ (import.meta).env;
    return env?.DEV ?? true;
  } catch {
    return true;
  }
})();

/**
 * Replaces `{name}` placeholders with values from `params`. Unknown, null and undefined
 * placeholders are left as is.
 * @param {string} template
 * @param {Record<string, string | number | null | undefined>} [params]
 * @returns {string}
 */
export function interpolate(template, params) {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name) =>
    Object.hasOwn(params, name) && params[name] != null ? String(params[name]) : match,
  );
}

const ACCENTS = /** @type {Record<string, string>} */ ({
  a: "á",
  e: "é",
  i: "í",
  o: "ó",
  u: "ú",
  c: "ç",
  n: "ñ",
  y: "ý",
  s: "š",
  z: "ž",
  A: "Á",
  E: "É",
  I: "Í",
  O: "Ó",
  U: "Ú",
  C: "Ç",
  N: "Ñ",
  Y: "Ý",
  S: "Š",
  Z: "Ž",
});

// A build with `PUBLIC_PSEUDO=1` starts in the pseudo-locale (the marketing layout check at +30%).
let pseudo = ["1", "true"].includes(String(/** @type {any} */ (import.meta).env?.PUBLIC_PSEUDO));

/**
 * Switches `t()` to the pseudo-locale: every message is accented and padded to at least 130%
 * of its length, so layouts can be checked for clipping before translations exist.
 * @param {boolean} on
 */
export function setPseudo(on) {
  pseudo = Boolean(on);
}

/** Whether the pseudo-locale is on. */
export const isPseudo = () => pseudo;

/**
 * The pseudo-locale form of a template: letters accented, `{name}` placeholders kept, and
 * bracketed padding that adds at least 30% of the template's length.
 * @param {string} template
 * @returns {string}
 */
export function pseudoize(template) {
  const accented = template
    .split(/(\{\w+\})/)
    .map((part, i) => (i % 2 === 1 ? part : part.replace(/[a-zA-Z]/g, (c) => ACCENTS[c] ?? c)))
    .join("");
  const extra = Math.ceil(template.length * 0.3);
  return `[${accented} ${"~".repeat(Math.max(0, extra - 3))}]`;
}

/**
 * Looks up a message by key and interpolates its placeholders.
 * Throws on an unknown key in dev; returns the key itself in production builds.
 * @param {MessageKey} key
 * @param {Record<string, string | number | null | undefined>} [params]
 * @returns {string}
 */
export function t(key, params) {
  const template = Object.hasOwn(messages, key) ? messages[key] : undefined;
  if (template === undefined) {
    if (isDev) throw new Error(`unknown message key: ${key}`);
    return key;
  }
  return interpolate(pseudo ? pseudoize(template) : template, params);
}
