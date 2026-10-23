// Form helpers shared by every surface.

import { isDev } from "./messages/index.js";

/**
 * Focuses the first field marked `aria-invalid="true"` inside `form`, in document order.
 * @param {ParentNode | null | undefined} form
 * @returns {HTMLElement | null} the focused field, or null when every field is valid or the
 *   field could not take focus
 */
export function focusFirstInvalid(form) {
  const field = form?.querySelector('[aria-invalid="true"]');
  if (!field || typeof (/** @type {any} */ (field).focus) !== "function") return null;
  /** @type {HTMLElement} */ (field).focus();
  // Report success only if focus really moved (a hidden or inert field refuses it).
  const doc = /** @type {any} */ (field).ownerDocument ?? globalThis.document;
  return doc?.activeElement === field ? /** @type {HTMLElement} */ (field) : null;
}

/**
 * Dev guard for `Button`: a disabled control must say why. Throws in dev builds only.
 * @param {boolean | undefined} disabled
 * @param {string | undefined} reason
 * @param {string} label
 * @param {boolean} [dev]
 */
export function assertDisabledReason(disabled, reason, label, dev = isDev) {
  if (dev && disabled && !reason) {
    throw new Error(`Button "${label}" is disabled without a disabledReason`); // i18n-ignore
  }
}
