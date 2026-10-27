import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  PLUGIN_BUILD,
  PLUGIN_DIR,
  adIds,
  cargoDependencyNames,
  checkArtefact,
  checkKotlinImports,
  checkPluginBuild,
  checkRust,
  checkSource,
  checkVariantIds,
  checkWindowsAndKiosk,
  lockPackageNames,
} from "./check-ads.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

function repo(files) {
  const dir = mkdtempSync(join(tmpdir(), "ads-"));
  for (const [path, text] of Object.entries(files)) {
    const full = join(dir, path);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, text);
  }
  return { dir, done: () => rmSync(dir, { recursive: true, force: true }) };
}

test("the repository passes the source check", () => {
  assert.deepEqual(checkSource(ROOT), []);
});

test("cargo dependency tables are read, including dotted and target tables", () => {
  const toml = `[package]
name = "x"
[dependencies]
serde = "1"
admob-sdk = "0.1"
[target.'cfg(target_os = "android")'.dependencies]
jni = "0.21"
[dev-dependencies.google-mobile-ads]
version = "1"
`;
  assert.deepEqual(cargoDependencyNames(toml), ["serde", "admob-sdk", "jni", "google-mobile-ads"]);
  assert.deepEqual(lockPackageNames('[[package]]\nname = "serde"\nversion = "1"\n'), ["serde"]);
});

test("a Rust crate that depends on an ads crate or names an ad endpoint fails", () => {
  const bad = repo({
    "Cargo.toml": "[workspace]\n",
    "crates/a/Cargo.toml": '[dependencies]\ngoogle-mobile-ads = "1"\n',
    "crates/a/src/lib.rs": 'const U: &str = "https://pagead2.googlesyndication.com/x";\n',
    "crates/b/src/lib.rs": "// calls com.google.android.gms.ads.MobileAds\n",
    "Cargo.lock": '[[package]]\nname = "admob"\n',
  });
  try {
    const errors = checkRust(bad.dir);
    assert.ok(errors.some((e) => e.includes("depends on the ads crate google-mobile-ads")));
    assert.ok(
      errors.some((e) => e.includes("crates/a/src/lib.rs") && e.includes("googlesyndication")),
    );
    assert.ok(
      errors.some((e) => e.includes("crates/b/src/lib.rs") && e.includes("ads SDK package")),
    );
    assert.ok(errors.some((e) => e.startsWith("Cargo.lock")));
  } finally {
    bad.done();
  }
  const ok = repo({
    "Cargo.toml": "[workspace]\n",
    "crates/a/Cargo.toml": '[dependencies]\nserde = "1"\n',
    "crates/a/src/lib.rs": "pub struct AdsConsent;\n",
  });
  try {
    assert.deepEqual(checkRust(ok.dir), []);
  } finally {
    ok.done();
  }
});

test("only the plugin module imports the ads SDK", () => {
  const r = repo({
    [`${PLUGIN_DIR}/android/src/A.kt`]: "import com.google.android.gms.ads.MobileAds\n",
    "apps/hub/src-tauri/gen/android/app/src/main/java/in/arena360/hub/Main.kt":
      "import com.google.android.gms.ads.AdView\n",
    "apps/hub/src-tauri/gen/android/app/build.gradle.kts":
      'implementation("com.google.android.gms:play-services-ads:25.5.0")\n',
  });
  try {
    const errors = checkKotlinImports(r.dir);
    assert.equal(errors.length, 2);
    assert.ok(errors.every((e) => !e.startsWith(PLUGIN_DIR)));
  } finally {
    r.done();
  }
});

test("the Kiosk and the Windows Hub shell carry no ad SDK, endpoint or plugin", () => {
  const r = repo({
    "apps/kiosk/src/a.ts": "window.ArenaAds?.init();\n",
    "apps/kiosk/src-tauri/Cargo.toml": '[dependencies]\ntauri-plugin-arena-ads = "1"\n',
    "apps/hub/src-tauri/tauri.conf.json": '{"plugins":{"arena-ads":{}}}\n',
    // The Hub UI's no-op gate may name the bridge, but not the SDK.
    "apps/hub/src/lib/ads.ts": "export const BRIDGE_NAME = 'ArenaAds';\n",
    "apps/hub/src/lib/bad.ts": "const u = 'https://googleads.g.doubleclick.net/';\n",
    // The Android project and the plugin module are where the SDK belongs.
    [`${PLUGIN_DIR}/android/build.gradle.kts`]: 'implementation("play-services-ads")\n',
    "apps/hub/src-tauri/gen/android/settings.gradle": "include ':tauri-plugin-arena-ads'\n",
  });
  try {
    const errors = checkWindowsAndKiosk(r.dir);
    assert.deepEqual(errors.map((e) => e.split(":")[0]).sort(), [
      "apps/hub/src-tauri/tauri.conf.json",
      "apps/hub/src/lib/bad.ts",
      "apps/hub/src/lib/bad.ts",
      "apps/kiosk/src-tauri/Cargo.toml",
      "apps/kiosk/src/a.ts",
    ]);
  } finally {
    r.done();
  }
});

const GOOD_BUILD = readFileSync(join(ROOT, PLUGIN_BUILD), "utf8");

test("the plugin build uses test units in debug and none in release", () => {
  assert.deepEqual(checkPluginBuild(GOOD_BUILD), []);
  assert.deepEqual(checkPluginBuild(undefined), [`${PLUGIN_BUILD} is missing`]);
  const prodDebug = GOOD_BUILD.replace(
    'buildConfigField("String", "BANNER_UNIT_ID", "\\"$testBannerUnit\\"")',
    'buildConfigField("String", "BANNER_UNIT_ID", "\\"ca-app-pub-1234567890123456/1234567890\\"")',
  );
  assert.notEqual(prodDebug, GOOD_BUILD);
  assert.ok(checkPluginBuild(prodDebug).some((e) => e.includes("debug BANNER_UNIT_ID")));
  const testRelease = GOOD_BUILD.replace(
    'val banner = releaseId("ARENA_ADMOB_BANNER_UNIT", "ca-app-pub-0000000000000000/0000000000")',
    'val banner = testBannerUnit; val leak = "ca-app-pub-3940256099942544/1"',
  );
  assert.ok(checkPluginBuild(testRelease).some((e) => e.includes("release")));
});

test("a debug variant may carry only test ids and a release variant none", () => {
  const test_ = {
    units: ["ca-app-pub-3940256099942544/9214589741", "ca-app-pub-3940256099942544/1033173712"],
    app: ["ca-app-pub-3940256099942544~3347511713"],
    stray: [],
  };
  const prod = {
    units: ["ca-app-pub-1111111111111111/2222222222"],
    app: ["ca-app-pub-1111111111111111~3333333333"],
    stray: [],
  };
  const unset = { units: ["ca-app-pub-0000000000000000/0000000000"], app: [], stray: [] };
  assert.deepEqual(checkVariantIds("debug", test_), []);
  assert.deepEqual(checkVariantIds("release", prod, true), []);
  assert.equal(checkVariantIds("debug", prod).length, 2);
  assert.equal(checkVariantIds("release", test_).length, 3);
  assert.deepEqual(checkVariantIds("release", unset), []);
  assert.equal(checkVariantIds("release", unset, true).length, 1);
  assert.equal(checkVariantIds("release", { units: [], app: [], stray: [] }).length, 1);
});

test("ad ids are read from BuildConfig and the merged manifest", () => {
  const found = adIds({
    buildConfig:
      'public static final String BANNER_UNIT_ID = "ca-app-pub-3940256099942544/9214589741";\n' +
      'public static final String INTERSTITIAL_UNIT_ID = "ca-app-pub-3940256099942544/1033173712";',
    manifest:
      '<meta-data android:name="com.google.android.gms.ads.APPLICATION_ID" android:value="ca-app-pub-3940256099942544~3347511713"/>',
  });
  assert.equal(found.units.length, 2);
  assert.deepEqual(found.app, ["ca-app-pub-3940256099942544~3347511713"]);
  assert.equal(found.stray.length, 3);
});

test("a built artefact with an ad string fails, as UTF-8 or UTF-16", () => {
  const r = repo({
    "ok/a.js": "console.log('hello')",
    "bad/utf8.js": "x = 'com.google.android.gms.ads'",
  });
  writeFileSync(join(r.dir, "bad/utf16.bin"), Buffer.from("ca-app-pub-", "utf16le"));
  try {
    assert.deepEqual(checkArtefact(join(r.dir, "ok")), []);
    assert.equal(checkArtefact(join(r.dir, "bad")).length, 2);
    assert.deepEqual(checkArtefact(join(r.dir, "bad/utf8.js"), ["com.google.android.gms.ads"]), []);
  } finally {
    r.done();
  }
});
