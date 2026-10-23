// The English message catalogue and `t()`. All user-visible text comes from `en.json`.

import en from "./en.json" with { type: "json" };

/** @typedef {keyof typeof en} MessageKey */

/** @type {Record<string, string>} */
export const messages = en;

const isDev = (() => {
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
  return interpolate(template, params);
}
