#!/usr/bin/env node
// Ad-code lint (Story 4.10). Ads reach the Android Hub only through one Kotlin module,
// `tauri-plugin-arena-ads`, which is the only code that imports the ads SDK. This check fails when:
//   source    a Rust crate depends on or mentions an ads SDK, symbol or endpoint; Kotlin or Java outside
//             the plugin imports the ads SDK; the Windows Hub, the Kiosk or the shared UI carry an
//             ad SDK or endpoint (the Kiosk also may not name the plugin); the plugin's debug build
//             type does not use Google's test unit ids or its release one does.
//   artefact  a built file (a Kiosk build, the Windows Hub executable or installer) contains one.
//   ids       a built Android variant carries the wrong ad ids: a test id in release, a production
//             id in debug (`--require-production` also refuses the all-zero placeholder).
// Reads local files only (no network).

import { existsSync, readdirSync, readFileSync, realpathSync, statSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const PLUGIN_DIR = "apps/hub/src-tauri/plugins/tauri-plugin-arena-ads";
export const PLUGIN_BUILD = `${PLUGIN_DIR}/android/build.gradle.kts`;
export const ANDROID_PROJECT = "apps/hub/src-tauri/gen/android";

/** Google's published test publisher id: every id under it is a test id. */
export const TEST_PUBLISHER = "ca-app-pub-3940256099942544";
export const UNSET_PUBLISHER = "ca-app-pub-0000000000000000";

/** SDK and endpoint strings no Rust crate, Windows build, Kiosk or shared UI may contain. */
export const SDK_PATTERNS = [
  [/com\.google\.android\.gms\.ads/i, "the ads SDK package"],
  [/play-services-ads/i, "the ads SDK artifact"],
  [/googlesyndication/i, "an ad endpoint (googlesyndication)"],
  [/doubleclick/i, "an ad endpoint (doubleclick)"],
  [/googleads/i, "an ad endpoint (googleads)"],
  [/\bpagead\d?\b/i, "an ad endpoint (pagead)"],
  [/\badmob\b/i, "AdMob"],
  [/\bca-app-pub-\d/i, "an ad unit or app id"],
];
/** Names of the plugin itself (the Kiosk and Rust may not mention it). */
export const PLUGIN_PATTERNS = [
  [/arena[-_]ads/i, "the ads plugin"],
  [/\bArenaAds\b/, "the ads plugin bridge"],
];
/** Crate names that are ad SDKs or bindings. */
export const ADS_CRATE =
  /^(.*ad-?mob.*|.*google-?mobile-?ads.*|.*gms-?ads.*|.*play-services-ads.*|.*arena-?ads.*)$/i;

const SKIP_DIRS = new Set([
  "node_modules",
  "target",
  "build",
  "dist",
  ".svelte-kit",
  ".git",
  ".gradle",
  "test-results",
]);

function* walk(dir, skip = SKIP_DIRS) {
  if (!existsSync(dir)) return;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    if (e.isDirectory()) {
      if (!skip.has(e.name)) yield* walk(join(dir, e.name), skip);
    } else yield join(dir, e.name);
  }
}

const rel = (root, p) => relative(root, p).split(sep).join("/");
const read = (p) => readFileSync(p, "utf8");

/** Each pattern that matches `text`, as `label` strings. */
export function findPatterns(text, patterns) {
  return patterns.filter(([re]) => re.test(text)).map(([, label]) => label);
}

/** Crate names a Cargo.toml lists as dependencies (any table whose name ends in `dependencies`). */
export function cargoDependencyNames(toml) {
  const names = [];
  let inDeps = false;
  for (const raw of toml.split(/\r?\n/)) {
    const line = raw.trim();
    const table = line.match(/^\[(.+)\]$/);
    if (table) {
      const head = table[1].trim();
      const sub = head.match(/dependencies\.(.+)$/);
      if (sub) {
        names.push(sub[1].replace(/"/g, ""));
        inDeps = false;
      } else inDeps = /dependencies$/.test(head);
      continue;
    }
    if (!inDeps || !line || line.startsWith("#")) continue;
    const m = line.match(/^([A-Za-z0-9_.-]+)\s*=/);
    if (m) names.push(m[1]);
  }
  return names;
}

/** Crate names a Cargo.lock lists (`name = "…"` under `[[package]]`). */
export function lockPackageNames(lock) {
  return [...lock.matchAll(/^name = "([^"]+)"/gm)].map((m) => m[1]);
}

const RUST_ROOTS = ["crates", "apps"];

/** No Rust crate depends on, or has symbols or endpoints of, an ads SDK or the plugin. */
export function checkRust(root) {
  const errors = [];
  const lock = join(root, "Cargo.lock");
  if (existsSync(lock)) {
    for (const n of lockPackageNames(read(lock))) {
      if (ADS_CRATE.test(n))
        errors.push(`Cargo.lock: the ads crate ${n} is in the dependency graph`);
    }
  }
  const files = [join(root, "Cargo.toml")];
  for (const top of RUST_ROOTS) {
    for (const f of walk(join(root, top))) {
      if (rel(root, f).startsWith(`${ANDROID_PROJECT}/`)) continue;
      if (rel(root, f).startsWith(`${PLUGIN_DIR}/`)) continue;
      if (/\.(rs|toml)$/.test(f)) files.push(f);
    }
  }
  for (const f of files) {
    if (!existsSync(f)) continue;
    const text = read(f);
    if (f.endsWith(".toml")) {
      for (const n of cargoDependencyNames(text)) {
        if (ADS_CRATE.test(n)) errors.push(`${rel(root, f)}: depends on the ads crate ${n}`);
      }
    }
    for (const what of findPatterns(text, [...SDK_PATTERNS, ...PLUGIN_PATTERNS])) {
      errors.push(`${rel(root, f)}: contains ${what}`);
    }
  }
  return errors;
}

/** Only the plugin module imports the ads SDK. */
export function checkKotlinImports(root) {
  const errors = [];
  for (const f of walk(join(root, "apps"))) {
    if (!/\.(kt|java|kts|gradle)$/.test(f)) continue;
    const r = rel(root, f);
    if (r.startsWith(`${PLUGIN_DIR}/`)) continue;
    const text = read(f);
    if (/com\.google\.android\.gms\.ads/.test(text)) {
      errors.push(`${r}: imports the ads SDK outside ${PLUGIN_DIR}`);
    }
    if (/play-services-ads/.test(text)) {
      errors.push(`${r}: depends on the ads SDK outside ${PLUGIN_DIR}`);
    }
  }
  return errors;
}

/**
 * The Windows Hub's and the Kiosk's Tauri projects and sources, and the shared UI sources, carry no
 * ad SDK or endpoint; the Kiosk (not the Hub UI, which holds the no-op gate) also never names the plugin.
 */
export function checkWindowsAndKiosk(root) {
  const errors = [];
  const hubShell = "apps/hub/src-tauri";
  const scopes = [
    { dir: "apps/kiosk", plugin: true },
    { dir: "apps/hub", plugin: false },
    { dir: "packages/ui", plugin: true },
  ];
  for (const { dir, plugin } of scopes) {
    for (const f of walk(join(root, dir))) {
      const r = rel(root, f);
      if (r.startsWith(`${ANDROID_PROJECT}/`) || r.startsWith(`${PLUGIN_DIR}/`)) continue;
      // Generated from DESIGN.md: its rule text names the ad product, it is not code.
      if (r.startsWith("packages/ui/src/tokens/")) continue;
      if (!/\.(rs|toml|json|ts|js|mjs|svelte|css|html|nsh|nsi|wxs|md)$/.test(f)) continue;
      if (/(^|\/)(pnpm-lock|package-lock)\.json$/.test(r)) continue;
      // The Hub UI's gate and its dev gallery name the plugin's bridge on purpose; the Hub's Tauri
      // shell (Cargo, config, capabilities, Rust) may not.
      const isHubUi = r.startsWith("apps/hub/") && !r.startsWith(`${hubShell}/`);
      const patterns = plugin || !isHubUi ? [...SDK_PATTERNS, ...PLUGIN_PATTERNS] : SDK_PATTERNS;
      const text = read(f);
      for (const what of findPatterns(text, patterns)) {
        errors.push(`${r}: contains ${what}`);
      }
    }
  }
  return errors;
}

/** The plugin's debug build type uses only Google's test units; its release one none of them. */
export function checkPluginBuild(text) {
  if (text === undefined) return [`${PLUGIN_BUILD} is missing`];
  const errors = [];
  const block = (name) => {
    const at = text.search(new RegExp(String.raw`getByName\("${name}"\)\s*\{`));
    if (at < 0) return undefined;
    let depth = 0;
    for (let i = text.indexOf("{", at); i < text.length; i++) {
      if (text[i] === "{") depth++;
      else if (text[i] === "}" && --depth === 0) return text.slice(at, i);
    }
    return undefined;
  };
  const constants = new Map(
    [...text.matchAll(/^val\s+(\w+)\s*=\s*"([^"]+)"/gm)].map((m) => [m[1], m[2]]),
  );
  const idsIn = (b) => {
    const ids = [];
    for (const m of b.matchAll(/buildConfigField\("String",\s*"(\w+)",\s*"\\"([^"\\]*)\\""\)/g)) {
      ids.push([m[1], constants.get(m[2].replace(/^\$/, "")) ?? m[2]]);
    }
    return ids;
  };
  const debug = block("debug");
  const release = block("release");
  if (debug === undefined) errors.push("plugin build: no debug build type");
  else {
    for (const field of ["BANNER_UNIT_ID", "INTERSTITIAL_UNIT_ID"]) {
      const id = idsIn(debug).find(([f]) => f === field)?.[1];
      if (!id?.startsWith(`${TEST_PUBLISHER}/`)) {
        errors.push(`plugin build: debug ${field} must be one of Google's test ad units`);
      }
    }
  }
  if (release === undefined) errors.push("plugin build: no release build type");
  else {
    if (release.includes(TEST_PUBLISHER)) {
      errors.push("plugin build: the release build type names a test ad unit");
    }
    for (const field of ["BANNER_UNIT_ID", "INTERSTITIAL_UNIT_ID"]) {
      const id = idsIn(release).find(([f]) => f === field)?.[1];
      if (id === undefined) errors.push(`plugin build: release ${field} is not set`);
      else if (id.startsWith(`${TEST_PUBLISHER}/`)) {
        errors.push(`plugin build: release ${field} is a test ad unit`);
      }
    }
  }
  return errors;
}

export function checkSource(root) {
  return [
    ...checkRust(root),
    ...checkKotlinImports(root),
    ...checkWindowsAndKiosk(root),
    ...checkPluginBuild(
      existsSync(join(root, PLUGIN_BUILD)) ? read(join(root, PLUGIN_BUILD)) : undefined,
    ),
  ];
}

/** Whether `buf` holds `needle` as UTF-8 or UTF-16LE text (binary manifests use either). */
function holds(buf, needle) {
  return buf.includes(Buffer.from(needle, "utf8")) || buf.includes(Buffer.from(needle, "utf16le"));
}

const ARTEFACT_STRINGS = [
  "com.google.android.gms.ads",
  "play-services-ads",
  "googlesyndication",
  "doubleclick",
  "googleads",
  "pagead2",
  "ca-app-pub-",
  "tauri-plugin-arena-ads",
  "arena-ads",
  "arena_ads",
  "ArenaAds",
];

/**
 * A built artefact (a Kiosk build folder, the Windows Hub executable or installer) must hold no ad
 * SDK, endpoint or plugin string. `allow` lists strings to ignore (the Hub's own web build holds
 * the no-op gate, which names the plugin's bridge).
 */
export function checkArtefact(path, allow = []) {
  const errors = [];
  const files = statSync(path).isDirectory() ? [...walk(path, new Set())] : [path];
  for (const f of files) {
    const buf = readFileSync(f);
    for (const s of ARTEFACT_STRINGS) {
      if (allow.includes(s)) continue;
      if (holds(buf, s)) errors.push(`${f}: contains "${s}"`);
    }
  }
  return errors;
}

const ID_RE = /ca-app-pub-\d{16}[~/]\d{10}/g;

/** The ad ids in a generated `BuildConfig.java` and a merged manifest. */
export function adIds({ buildConfig = "", manifest = "" }) {
  return {
    units: [
      ...buildConfig.matchAll(/(?:BANNER_UNIT_ID|INTERSTITIAL_UNIT_ID)\s*=\s*"([^"]*)"/g),
    ].map((m) => m[1]),
    app: [
      ...manifest.matchAll(/com\.google\.android\.gms\.ads\.APPLICATION_ID"[^>]*?value="([^"]*)"/g),
    ].map((m) => m[1]),
    stray: [...(buildConfig + manifest).matchAll(ID_RE)].map((m) => m[0]),
  };
}

/**
 * Debug carries only Google's test ids, release none of them (and, with `requireProduction`, no
 * all-zero placeholder either).
 */
export function checkVariantIds(variant, found, requireProduction = false) {
  const errors = [];
  const all = [...found.units, ...found.app];
  if (all.length === 0) return [`${variant}: no ad ids found in the build output`];
  for (const id of all) {
    const isTest = id.startsWith(TEST_PUBLISHER);
    if (variant === "debug" && !isTest) {
      errors.push(`debug build contains the production ad id ${id}`);
    }
    if (variant === "release") {
      if (isTest) errors.push(`release build contains the test ad id ${id}`);
      else if (requireProduction && id.startsWith(UNSET_PUBLISHER)) {
        errors.push(`release build still has the placeholder ad id ${id}`);
      }
    }
  }
  return errors;
}

export function checkBuiltIds(root, requireProduction = false) {
  const errors = [];
  const cfgRoot = join(root, PLUGIN_DIR, "android/build/generated/source/buildConfig");
  const manifestRoot = join(root, ANDROID_PROJECT, "app/build/intermediates/merged_manifests");
  let seen = 0;
  for (const variant of ["debug", "release"]) {
    const cfg = [...walk(join(cfgRoot, variant), new Set())].find((f) =>
      f.endsWith("BuildConfig.java"),
    );
    const manifest = [...walk(join(manifestRoot, variant), new Set())].find((f) =>
      f.endsWith("AndroidManifest.xml"),
    );
    if (!cfg && !manifest) continue;
    seen++;
    errors.push(
      ...checkVariantIds(
        variant,
        adIds({
          buildConfig: cfg ? read(cfg) : "",
          manifest: manifest ? read(manifest) : "",
        }),
        requireProduction,
      ),
    );
  }
  if (seen === 0)
    errors.push("no built variant found: build the debug and release Android Hub first");
  return errors;
}

function main() {
  const args = process.argv.slice(2);
  const [mode, ...rest] = args;
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  let errors;
  if (mode === "source") errors = checkSource(root);
  else if (mode === "ids") errors = checkBuiltIds(root, rest.includes("--require-production"));
  else if (mode === "artefact" && rest.length > 0) {
    const allow = rest.filter((a) => a.startsWith("--allow=")).map((a) => a.slice(8));
    errors = rest.filter((a) => !a.startsWith("--")).flatMap((p) => checkArtefact(p, allow));
  } else {
    console.error(
      "usage: check-ads.mjs source | ids [--require-production] | artefact <path>... [--allow=<string>]",
    );
    process.exit(64);
  }
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log(`ads ${mode} check: ok`);
}

const isMain = (() => {
  try {
    return (
      Boolean(process.argv[1]) &&
      realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1])
    );
  } catch {
    return false;
  }
})();

if (isMain) main();
