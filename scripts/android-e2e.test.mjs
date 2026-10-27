import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { failureMessage, lastStep, steps } from "./android-e2e.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const WORKFLOW = readFileSync(join(ROOT, ".github/workflows/android-e2e.yml"), "utf8");

const LOG = [
  "running 1 test",
  "android-e2e step: sign-up with phone OTP",
  "android-e2e step: pair the Android Hub with the same code",
  "thread 'x' panicked at tests/free_android_golive.rs:1:1",
].join("\n");

test("steps are read in order and the last one is the failure", () => {
  assert.deepEqual(steps(LOG), [
    "sign-up with phone OTP",
    "pair the Android Hub with the same code",
  ]);
  assert.equal(lastStep(LOG), "pair the Android Hub with the same code");
  assert.equal(
    failureMessage(LOG),
    "android-e2e failed at step: pair the Android Hub with the same code",
  );
});

test("a log with no step fails before the first step", () => {
  assert.equal(lastStep("error: could not compile"), null);
  assert.equal(failureMessage(""), "android-e2e failed before the first step");
});

test("CRLF logs parse the same", () => {
  assert.deepEqual(steps(LOG.replaceAll("\n", "\r\n")), steps(LOG));
});

test("the workflow runs nightly and on release candidates", () => {
  assert.match(WORKFLOW, /schedule:\s*\n\s*- cron:/);
  assert.match(WORKFLOW, /tags:\s*\[[^\]]*v\*-rc\*/);
  assert.match(WORKFLOW, /workflow_dispatch:/);
});

test("a failing run names the step and keeps a screenshot", () => {
  assert.match(WORKFLOW, /scripts\/android-e2e\.mjs failed-step/);
  assert.match(WORKFLOW, /actions\/upload-artifact@v4[\s\S]*screenshot/);
  assert.match(WORKFLOW, /if: failure\(\)/);
});

test("the workflow runs the Free-café end-to-end test", () => {
  assert.match(WORKFLOW, /--test free_android_golive/);
});
