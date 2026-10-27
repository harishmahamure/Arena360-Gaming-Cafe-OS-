// Helpers for the `android-e2e` workflow (Story 4.8). The Free-café end-to-end test announces
// each step as `android-e2e step: <name>`; when the run fails, `failed-step` names the step
// that was running, so the failure says which step broke.
//
//   node scripts/android-e2e.mjs failed-step <test-log>   prints "android-e2e failed at step: <name>"
import { readFileSync, appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const MARK = "android-e2e step: ";

/** The steps announced in `log`, in order. */
export function steps(log) {
  return log
    .split(/\r?\n/)
    .filter((l) => l.startsWith(MARK))
    .map((l) => l.slice(MARK.length).trim());
}

/** The step that was running when the log ended, or null if none was announced. */
export function lastStep(log) {
  return steps(log).at(-1) ?? null;
}

/** The one-line failure message, naming the step. */
export function failureMessage(log) {
  const step = lastStep(log);
  return step === null
    ? "android-e2e failed before the first step"
    : `android-e2e failed at step: ${step}`;
}

function main(argv) {
  const [cmd, file] = argv;
  if (cmd !== "failed-step" || !file) {
    console.error("usage: node scripts/android-e2e.mjs failed-step <test-log>");
    return 64;
  }
  const message = failureMessage(readFileSync(file, "utf8"));
  console.log(message);
  if (process.env.GITHUB_STEP_SUMMARY) {
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, `### ${message}\n`);
  }
  return 0;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
