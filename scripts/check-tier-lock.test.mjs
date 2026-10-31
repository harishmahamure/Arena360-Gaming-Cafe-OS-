import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { checkComponent, checkFlag, checkRepo, checkSource } from "./check-tier-lock.mjs";

test("the Available soon placeholder key fails with file:line", () => {
  const text = ["<p>", '  <span>{t("staff.tier.soon")}</span>', "</p>"].join("\n");
  assert.deepEqual(checkSource("apps/hub/src/x.svelte", text), [
    "apps/hub/src/x.svelte:2 available-soon-placeholder",
  ]);
});

test("a TierLock given the old billing switch fails", () => {
  assert.deepEqual(checkSource("apps/owner/src/a.svelte", '<TierLock tier="Premium" billing />'), [
    "apps/owner/src/a.svelte:1 billing-switch",
  ]);
  assert.deepEqual(
    checkSource("apps/owner/src/a.svelte", '<TierLock tier="Basic" branch={b.id} />'),
    [],
  );
});

test("the flag must be on and the component must link to the picker", () => {
  assert.deepEqual(checkFlag("export const billingEnabled = true;\n"), []);
  assert.equal(checkFlag("export const billingEnabled = false;\n").length, 1);
  assert.deepEqual(checkComponent('<a class="upgrade" href={link}>x</a>'), []);
  assert.equal(checkComponent("<span>soon</span>").length, 1);
});

test("this script and its test are exempt", () => {
  assert.deepEqual(checkSource("scripts/check-tier-lock.mjs", "staff.tier.soon"), []);
  assert.deepEqual(checkSource("scripts/check-tier-lock.test.mjs", "staff.tier.soon"), []);
});

test("the repository scan finds a stray placeholder under an app and ignores build output", () => {
  const root = mkdtempSync(join(tmpdir(), "tier-lock-"));
  try {
    const put = (file, text) => {
      mkdirSync(dirname(join(root, file)), { recursive: true });
      writeFileSync(join(root, file), text);
    };
    put("packages/ui/src/billing.js", "export const billingEnabled = true;\n");
    put("packages/ui/src/components/TierLock.svelte", '<a class="upgrade" href={l}>Upgrade</a>');
    put("apps/hub/src/ok.svelte", "<p>ok</p>");
    put("apps/hub/build/old.js", 't("staff.tier.soon")');
    assert.deepEqual(checkRepo(root), []);
    put("apps/hub/src/bad.svelte", '{t("staff.tier.soon")}');
    assert.deepEqual(checkRepo(root), ["apps/hub/src/bad.svelte:1 available-soon-placeholder"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
