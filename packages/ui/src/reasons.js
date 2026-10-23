// The reason codes each intent kind requires, from `IntentKind::reasons()` in arena-proto
// (generated as `generated/reasons.json`), so a screen can ask for the reason with the PIN
// instead of learning it from a REASON_REQUIRED refusal.

import reasons from "./generated/reasons.json" with { type: "json" };

/** @type {Record<string, string[]>} */
const TABLE = reasons;

/**
 * The reason codes `kind` requires, or `null` when it takes none.
 * @param {string} kind an intent kind, e.g. `device.revoke`
 * @returns {import("./generated/index").ReasonCode[] | null}
 */
export function reasonsFor(kind) {
  const codes = Object.hasOwn(TABLE, kind) ? TABLE[kind] : null;
  return /** @type {any} */ (codes ? [...codes] : null);
}

/**
 * The ReasonPicker options for `kind`: each code with its `reason.<code>` catalogue key.
 * @param {string} kind
 * @returns {{ value: string, label: import("./messages/index.js").MessageKey }[]}
 */
export function reasonOptions(kind) {
  return (reasonsFor(kind) ?? []).map((code) => ({
    value: code,
    label: /** @type {import("./messages/index.js").MessageKey} */ (`reason.${code}`), // i18n-ignore (a catalogue key)
  }));
}
