import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// The Kiosk installer (Story 1.16): NSIS cannot read the message catalogue, so each refusal
// text in hooks.nsh duplicates its key and is checked here, together with the service
// registration and the installer config.

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");
const TAURI = "apps/kiosk/src-tauri";
const hooks = () => read(`${TAURI}/windows/hooks.nsh`);
const catalogue = () => JSON.parse(read("packages/ui/src/messages/en.json"));

/** The body of `!macro name` … `!macroend`. */
const macro = (text, name) => {
  const m = text.match(new RegExp(`!macro ${name}\\b([\\s\\S]*?)!macroend`));
  assert.ok(m, `${name} is defined`);
  return m[1];
};

/** Net pushes minus pops along a list of lines (`!insertmacro` expands the service macro). */
const stackDelta = (text, lines) =>
  lines.reduce((n, line) => {
    const l = line.replace(/;.*$/, "").trim();
    if (/^Push\b/.test(l)) return n + 1;
    if (/^Pop\b/.test(l)) return n - 1;
    if (/^nsExec::Exec(ToLog)?\b/.test(l)) return n + 1;
    if (/^!insertmacro ARENA_REMOVE_KIOSK_SERVICE$/.test(l)) {
      return n + stackDelta(text, macro(text, "ARENA_REMOVE_KIOSK_SERVICE").split("\n"));
    }
    return n;
  }, 0);

test("every refusal names its requirement with the catalogue text", () => {
  const text = hooks();
  const en = catalogue();
  for (const key of [
    "kiosk.installer.windows_too_old",
    "kiosk.installer.not_64bit",
    "kiosk.installer.ram_too_low",
    "kiosk.installer.service_failed",
  ]) {
    assert.ok(en[key], `${key} is in en.json`);
    assert.ok(text.includes(`key "${key}"`), `hooks.nsh names ${key}`);
    assert.ok(text.includes(`"${en[key]}"`), `hooks.nsh carries the text of ${key}`);
  }
  const pre = macro(text, "NSIS_HOOK_PREINSTALL");
  assert.match(pre, /CurrentBuildNumber/);
  assert.match(pre, /IntCmp \$0 17134 (\w+) (\w+) \1/, "below 17134 jumps to the refusal");
  assert.match(pre, /\$\{IfNot\} \$\{RunningX64\}/);
  assert.match(pre, /GetPhysicallyInstalledSystemMemory/);
  assert.match(pre, /IntCmp \$0 4194304 (\w+) (\w+) \1/, "below 4 GB (in KB) is refused");
  assert.equal((pre.match(/^\s*Abort\b/gm) ?? []).length, 3);
});

test("the service is removed before an upgrade or uninstall and registered after install", () => {
  const text = hooks();
  const remove = macro(text, "ARENA_REMOVE_KIOSK_SERVICE");
  assert.match(remove, /nsExec::Exec '"\$INSTDIR\\arena-kiosk-svc\.exe" service uninstall'/);
  const pre = macro(text, "NSIS_HOOK_PREINSTALL");
  assert.ok(
    pre.indexOf("!insertmacro ARENA_REMOVE_KIOSK_SERVICE") > pre.indexOf("arena_ram_ok:"),
    "the old service is removed only once every check has passed",
  );
  assert.match(
    macro(text, "NSIS_HOOK_POSTINSTALL"),
    /nsExec::ExecToLog '"\$INSTDIR\\arena-kiosk-svc\.exe" service install'/,
  );
  assert.match(macro(text, "NSIS_HOOK_PREUNINSTALL"), /!insertmacro ARENA_REMOVE_KIOSK_SERVICE/);
});

test("every hook path leaves the NSIS stack balanced", () => {
  const text = hooks();
  const pre = macro(text, "NSIS_HOOK_PREINSTALL").split("\n");
  // Each refusal: from the start to its Abort, the branches before it taken as passed.
  const aborts = pre.flatMap((l, i) => (/^\s*Abort\b/.test(l) ? [i] : []));
  const skip = (lines, from, label) => {
    const start = lines.findIndex((l, i) => i >= from && l.trim() === label);
    return start;
  };
  const okLabels = ["arena_build_ok:", "${EndIf}", "arena_ram_ok:"];
  const starts = ["arena_build_too_old:", "${IfNot} ${RunningX64}", "arena_ram_too_low:"];
  let passed = [];
  let cursor = 0;
  aborts.forEach((abort, k) => {
    const blockStart = pre.findIndex((l, i) => i >= cursor && l.trim() === starts[k]);
    assert.ok(blockStart >= 0 && blockStart < abort, `refusal ${k} found`);
    assert.equal(
      stackDelta(text, [
        ...passed,
        ...pre.slice(cursor, blockStart),
        ...pre.slice(blockStart + 1, abort),
      ]),
      0,
      `balanced before Abort ${k}`,
    );
    passed = [...passed, ...pre.slice(cursor, blockStart)];
    cursor = skip(pre, abort, okLabels[k]);
    assert.ok(cursor > abort, `the ok label after refusal ${k}`);
  });
  assert.equal(stackDelta(text, [...passed, ...pre.slice(cursor)]), 0, "balanced on success");
  assert.equal(stackDelta(text, macro(text, "NSIS_HOOK_POSTINSTALL").split("\n")), 0);
  assert.equal(stackDelta(text, macro(text, "NSIS_HOOK_PREUNINSTALL").split("\n")), 0);
});

test("the installer config bundles the service and embeds the WebView2 bootstrapper", () => {
  const conf = JSON.parse(read(`${TAURI}/tauri.installer.conf.json`));
  assert.equal(conf.bundle.active, true);
  assert.deepEqual(conf.bundle.targets, ["nsis"]);
  assert.deepEqual(conf.bundle.externalBin, ["binaries/arena-kiosk-svc"]);
  assert.deepEqual(conf.bundle.windows.webviewInstallMode, { type: "embedBootstrapper" });
  assert.equal(conf.bundle.windows.nsis.installerHooks, "windows/hooks.nsh");
  assert.equal(conf.bundle.windows.nsis.installMode, "perMachine", "a service needs admin");
});

test("the base Kiosk config builds without the service and covers the screen", () => {
  const conf = JSON.parse(read(`${TAURI}/tauri.conf.json`));
  assert.equal(conf.bundle.externalBin, undefined);
  const main = conf.app.windows.find((w) => w.label === "main");
  assert.equal(main.fullscreen, true);
  assert.equal(main.decorations, false);
  // The webview reaches nothing but its own bundle (no Hub socket).
  assert.match(conf.app.security.csp, /^default-src 'self';/);
  assert.doesNotMatch(conf.app.security.csp, /connect-src/);
  assert.match(read(".gitignore"), /^apps\/kiosk\/src-tauri\/binaries\/$/m);
});
