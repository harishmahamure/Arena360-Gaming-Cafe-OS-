import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// The Hub installer and Tauri shell config: NSIS cannot read the message catalogue, so the
// minimum-Windows text in hooks.nsh duplicates its key and is checked here.

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");
const TAURI = "apps/hub/src-tauri";
const KEY = "hub.installer.windows_too_old";

test("hooks.nsh refuses Windows builds older than 17134 with the catalogue text", () => {
  const hooks = read(`${TAURI}/windows/hooks.nsh`);
  const message = JSON.parse(read("packages/ui/src/messages/en.json"))[KEY];
  assert.ok(message, `${KEY} is in en.json`);
  assert.match(hooks, /!macro NSIS_HOOK_PREINSTALL[\s\S]*!macroend/);
  assert.match(hooks, /CurrentBuildNumber/);
  assert.match(hooks, /IntCmp \$0 17134 (\w+) (\w+) \1/, "less than 17134 jumps to the refusal");
  assert.ok(hooks.includes(`"${message}"`), "the refusal text matches en.json");
  assert.ok(hooks.includes(KEY), "the hook names the catalogue key");
  assert.match(hooks, /\bAbort\b/);
});

/** The body of `!macro name` … `!macroend`. */
const macro = (hooks, name) => {
  const m = hooks.match(new RegExp(`!macro ${name}\\b([\\s\\S]*?)!macroend`));
  assert.ok(m, `${name} is defined`);
  return m[1];
};

/** Net pushes minus pops along a list of lines (`!insertmacro` expands the stop macro). */
const stackDelta = (hooks, lines) =>
  lines.reduce((n, line) => {
    const l = line.replace(/;.*$/, "").trim();
    if (/^Push\b/.test(l)) return n + 1;
    if (/^Pop\b/.test(l)) return n - 1;
    if (/^nsExec::Exec\b/.test(l)) return n + 1;
    if (/^!insertmacro ARENA_STOP_HUB_SERVICE$/.test(l)) {
      return n + stackDelta(hooks, macro(hooks, "ARENA_STOP_HUB_SERVICE").split("\n"));
    }
    return n;
  }, 0);

test("install and uninstall stop the running arena-hub service first", () => {
  const hooks = read(`${TAURI}/windows/hooks.nsh`);
  const stop = macro(hooks, "ARENA_STOP_HUB_SERVICE");
  assert.match(stop, /nsExec::Exec 'taskkill \/IM arena-hub\.exe \/F'/);
  assert.equal(stackDelta(hooks, stop.split("\n")), 0, "the exit code is popped");
  const pre = macro(hooks, "NSIS_HOOK_PREINSTALL");
  assert.ok(
    pre.indexOf("!insertmacro ARENA_STOP_HUB_SERVICE") > pre.indexOf("arena_windows_ok:"),
    "the service is stopped after the version check passes",
  );
  assert.match(macro(hooks, "NSIS_HOOK_PREUNINSTALL"), /!insertmacro ARENA_STOP_HUB_SERVICE/);
});

test("every hook path leaves the NSIS stack balanced", () => {
  const hooks = read(`${TAURI}/windows/hooks.nsh`);
  const pre = macro(hooks, "NSIS_HOOK_PREINSTALL").split("\n");
  const tooOld = pre.findIndex((l) => l.trim() === "arena_windows_too_old:");
  const abort = pre.findIndex((l) => /^\s*Abort\b/.test(l));
  const ok = pre.findIndex((l) => l.trim() === "arena_windows_ok:");
  assert.ok(tooOld >= 0 && abort > tooOld && ok > abort);
  // Refusal: from the start to Abort, skipping nothing (the IntCmp falls into it).
  assert.equal(stackDelta(hooks, pre.slice(0, abort)), 0, "balanced before Abort");
  // Success: the start up to the IntCmp, then from the ok label on.
  const intcmp = pre.findIndex((l) => /^\s*IntCmp\b/.test(l));
  assert.equal(stackDelta(hooks, [...pre.slice(0, intcmp + 1), ...pre.slice(ok)]), 0);
  assert.equal(stackDelta(hooks, macro(hooks, "NSIS_HOOK_PREUNINSTALL").split("\n")), 0);
});

test("the installer config bundles the arena-hub sidecar and the NSIS hooks", () => {
  const conf = JSON.parse(read(`${TAURI}/tauri.installer.conf.json`));
  assert.deepEqual(conf.bundle.externalBin, ["binaries/arena-hub"]);
  assert.deepEqual(conf.bundle.targets, ["nsis"]);
  assert.equal(conf.bundle.active, true);
  assert.equal(conf.bundle.windows.nsis.installerHooks, "windows/hooks.nsh");
});

test("the base Tauri config can build without the sidecar and may reach only the loopback Hub", () => {
  const conf = JSON.parse(read(`${TAURI}/tauri.conf.json`));
  // externalBin lives only in the installer config: tauri-build would otherwise need the
  // binary for every workspace build and copy it over target/<profile>/arena-hub.
  assert.equal(conf.bundle.externalBin, undefined);
  const csp = conf.app.security.csp;
  assert.match(csp, /connect-src 'self' http:\/\/127\.0\.0\.1:7409(;|$)/);
  const main = conf.app.windows.find((w) => w.label === "main");
  assert.equal(main.visible, false, "the window is shown once the service is up");
  assert.match(read(".gitignore"), /^apps\/hub\/src-tauri\/binaries\/$/m);
});
