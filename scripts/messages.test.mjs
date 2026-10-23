import { test } from "node:test";
import assert from "node:assert/strict";
import { interpolate, messages, setPseudo, t } from "../packages/ui/src/messages/index.js";

test("interpolation fills placeholders", () => {
  assert.equal(
    t("player.balance.insufficient", { balance: "₹40" }),
    "Balance ₹40. Top up at counter.",
  );
  assert.equal(
    t("staff.setup_code.issued", { name: "Neha", code: "K7Q4 9MZD" }),
    "Setup code for Neha: K7Q4 9MZD. Give it to them; it works once.",
  );
});

test("an unknown key throws in dev", () => {
  assert.throws(() => t(/** @type {any} */ ("staff.nope")), /unknown message key: staff.nope/);
});

test("missing params leave the placeholder; keys without params are returned as is", () => {
  assert.equal(interpolate("Balance {balance}.", {}), "Balance {balance}.");
  assert.equal(t("app.name"), "Arena 360");
});

test("the catalogue is flat dotted keys with string values", () => {
  for (const [key, value] of Object.entries(messages)) {
    assert.match(key, /^[a-z][a-z0-9_]*(\.[a-z0-9_]+)+$/, key);
    assert.equal(typeof value, "string", key);
  }
});

test("every surface has a name", () => {
  for (const s of [
    "hub",
    "kiosk",
    "kitchen",
    "owner",
    "partner",
    "admin",
    "onboarding",
    "marketing",
  ]) {
    assert.ok(messages[`surface.${s}`], s);
  }
});

test("null and undefined params leave the placeholder", () => {
  assert.equal(interpolate("Balance {balance}.", { balance: null }), "Balance {balance}.");
  assert.equal(interpolate("Balance {balance}.", { balance: undefined }), "Balance {balance}.");
  assert.equal(interpolate("{n} left", { n: 0 }), "0 left");
});

test("the pseudo-locale accents and lengthens every message by at least 30%", () => {
  setPseudo(true);
  try {
    for (const key of Object.keys(messages)) {
      const plain = messages[key];
      const out = t(/** @type {any} */ (key));
      assert.ok(out.length >= Math.ceil(plain.length * 1.3), `${key}: ${out}`);
      if (/[aeiou]/i.test(plain.replace(/\{\w+\}/g, ""))) assert.match(out, /[áéíóú]/i, key);
    }
    assert.equal(
      t("player.balance.insufficient", { balance: "₹40" }),
      "[Báláñçé ₹40. Tóp úp át çóúñtér. ~~~~~~~~~]",
    );
  } finally {
    setPseudo(false);
  }
  assert.equal(t("app.name"), "Arena 360");
});
