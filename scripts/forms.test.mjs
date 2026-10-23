import { test } from "node:test";
import assert from "node:assert/strict";
import { assertDisabledReason, focusFirstInvalid } from "../packages/ui/src/forms.js";

test("a disabled button without a reason throws in dev", () => {
  assert.throws(
    () => assertDisabledReason(true, undefined, "gallery.button.primary", true),
    /disabled without a disabledReason/,
  );
  assert.doesNotThrow(() =>
    assertDisabledReason(true, "gallery.disabled.closed", "gallery.button.primary", true),
  );
  assert.doesNotThrow(() => assertDisabledReason(false, undefined, "gallery.button.primary", true));
});

test("production builds do not throw for a missing reason", () => {
  assert.doesNotThrow(() => assertDisabledReason(true, undefined, "gallery.button.primary", false));
});

test("focusFirstInvalid focuses the first aria-invalid field", () => {
  const focused = [];
  const doc = { activeElement: null };
  const field = {
    ownerDocument: doc,
    focus: () => {
      focused.push("username");
      doc.activeElement = field;
    },
  };
  const form = {
    querySelector: (selector) => (selector === '[aria-invalid="true"]' ? field : null),
  };
  assert.equal(focusFirstInvalid(/** @type {any} */ (form)), field);
  assert.deepEqual(focused, ["username"]);
  assert.equal(focusFirstInvalid(/** @type {any} */ ({ querySelector: () => null })), null);
  assert.equal(focusFirstInvalid(null), null);
});

test("focusFirstInvalid returns null when the field refuses focus", () => {
  const doc = { activeElement: null };
  const field = { ownerDocument: doc, focus: () => {} };
  const form = { querySelector: () => field };
  assert.equal(focusFirstInvalid(/** @type {any} */ (form)), null);
});
