#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync, realpathSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const ANDROID_DIR = "apps/hub/src-tauri/gen/android/app/src/main";
export const PAGE_ALIGN = 16384n;
export const REQUIRED_ABIS = ["arm64-v8a", "x86_64"];
const PRIVATE_DOMAINS = ["root", "file", "database", "sharedpref", "external"];

const attr = (xml, name) => xml.match(new RegExp(`\\b${name}="([^"]*)"`))?.[1];

function excludes(block, domain) {
  return [...block.matchAll(/<exclude\b([^>]*)\/?>/g)].some((m) => {
    const a = (name) => m[1].match(new RegExp(`\\b${name}="([^"]*)"`))?.[1];
    return a("domain") === domain && a("path") === ".";
  });
}

function missingExcludes(label, block) {
  if (block === undefined) return [`${label}: section is missing`];
  return PRIVATE_DOMAINS.filter((d) => !excludes(block, d)).map(
    (d) => `${label}: does not exclude the ${d} domain`,
  );
}

const section = (xml, tag) => xml.match(new RegExp(`<${tag}>([\\s\\S]*?)</${tag}>`))?.[1];

export function checkManifest(manifest, res) {
  const errors = [];
  const rule = (name) => {
    const ref = attr(manifest, name);
    const file = ref?.replace(/^@xml\//, "");
    if (!file) errors.push(`manifest: ${name} is not set`);
    else if (res[file] === undefined)
      errors.push(`manifest: ${name} points to missing ${file}.xml`);
    return file ? res[file] : undefined;
  };
  if (attr(manifest, "android:allowBackup") !== "false") {
    errors.push('manifest: android:allowBackup must be "false"');
  }
  const extraction = rule("android:dataExtractionRules");
  if (extraction !== undefined) {
    for (const tag of ["cloud-backup", "device-transfer"]) {
      errors.push(...missingExcludes(`dataExtractionRules ${tag}`, section(extraction, tag)));
    }
  }
  const backup = rule("android:fullBackupContent");
  if (backup !== undefined) {
    errors.push(...missingExcludes("fullBackupContent", section(backup, "full-backup-content")));
  }
  const network = rule("android:networkSecurityConfig");
  if (network !== undefined) {
    if (!/<base-config\s+cleartextTrafficPermitted="false"/.test(network)) {
      errors.push("networkSecurityConfig: base-config must refuse cleartext");
    }
    const hosts = [...network.matchAll(/<domain[^>]*>([^<]+)<\/domain>/g)].map((m) => m[1].trim());
    const bad = hosts.filter((h) => h !== "127.0.0.1" && h !== "localhost");
    if (bad.length > 0)
      errors.push(`networkSecurityConfig: cleartext allowed for ${bad.join(", ")}`);
  }
  return errors;
}

export const SERVICE_PERMISSIONS = [
  "FOREGROUND_SERVICE",
  "FOREGROUND_SERVICE_CONNECTED_DEVICE",
  "WAKE_LOCK",
  "POST_NOTIFICATIONS",
  "RECEIVE_BOOT_COMPLETED",
  "REQUEST_IGNORE_BATTERY_OPTIMIZATIONS",
];
export const BOOT_ACTIONS = ["BOOT_COMPLETED", "MY_PACKAGE_REPLACED"];

const element = (xml, tag, name) =>
  [...xml.matchAll(new RegExp(`<${tag}\\b[^>]*?(?:/>|>[\\s\\S]*?</${tag}>)`, "g"))]
    .map((m) => m[0])
    .find((e) => attr(e, "android:name") === name);

export function checkService(manifest) {
  const errors = SERVICE_PERMISSIONS.filter(
    (p) =>
      !new RegExp(`<uses-permission\\b[^>]*android:name="android\\.permission\\.${p}"`).test(
        manifest,
      ),
  ).map((p) => `manifest: permission ${p} is missing`);
  const service = element(manifest, "service", ".HubService");
  if (service === undefined) errors.push("manifest: .HubService is not declared");
  else {
    if (attr(service, "android:foregroundServiceType") !== "connectedDevice") {
      errors.push('manifest: .HubService must have foregroundServiceType="connectedDevice"');
    }
    if (attr(service, "android:exported") !== "false") {
      errors.push('manifest: .HubService must have exported="false"');
    }
  }
  const receiver = element(manifest, "receiver", ".BootReceiver");
  if (receiver === undefined) errors.push("manifest: .BootReceiver is not declared");
  else {
    for (const a of BOOT_ACTIONS) {
      if (!receiver.includes(`android.intent.action.${a}"`)) {
        errors.push(`manifest: .BootReceiver does not listen for ${a}`);
      }
    }
  }
  return errors;
}

/**
 * Entries that appear twice in the manifest (a merge once left a second `<service>`,
 * `<receiver>` and every permission).
 */
export function checkDuplicates(manifest) {
  const seen = new Map();
  for (const m of manifest.matchAll(
    /<(uses-permission|service|receiver|activity|provider)(?=[\s>/])[^>]*>/g,
  )) {
    const name = attr(m[0], "android:name");
    if (name === undefined) continue;
    const key = `${m[1]} ${name}`;
    seen.set(key, (seen.get(key) ?? 0) + 1);
  }
  return [...seen]
    .filter(([, n]) => n > 1)
    .map(([key]) => `manifest: <${key}> is declared more than once`);
}

/**
 * The Keystore bridge (Kotlin) must keep the Hub's secrets key in the Android Keystore and
 * never read it out: the `AndroidKeyStore` provider, AES-GCM, and no call that exports or
 * reveals the key bytes, imports a key, or loosens its use.
 */
export const KEYSTORE_FORBIDDEN = [
  [/\.encoded\b|\bgetEncoded\s*\(/, "reads the key bytes (encoded / getEncoded)"],
  [/\bSecretKeySpec\b/, "builds a key from raw bytes (SecretKeySpec)"],
  [/setRandomizedEncryptionRequired\s*\(\s*false/, "turns off randomised encryption"],
  [/\bsetUserAuthenticationRequired\s*\(\s*false/, "loosens user authentication"],
  [/\bsetKeyEntry\b|\bsetEntry\b|\bPasswordProtection\b/, "imports a key into the Keystore"],
];

export function checkKeystore(source) {
  if (source === undefined) return ["keystore: KeystoreBridge.kt is missing"];
  const errors = [];
  if (!/"AndroidKeyStore"/.test(source)) {
    errors.push("keystore: the bridge must use the AndroidKeyStore provider");
  }
  if (!/AES\/GCM\/NoPadding/.test(source)) {
    errors.push("keystore: the bridge must use AES/GCM/NoPadding");
  }
  if (!/KeyGenParameterSpec\.Builder/.test(source)) {
    errors.push("keystore: the key must be generated in the Keystore (KeyGenParameterSpec)");
  }
  const code = source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, "");
  for (const [re, what] of KEYSTORE_FORBIDDEN) {
    if (re.test(code)) errors.push(`keystore: the bridge ${what}`);
  }
  return errors;
}

/**
 * The NsdManager bridge (Kotlin) is how kiosks find the Hub: it must register the Arena service
 * type, watch the default network, and the manifest must allow reading network state and
 * multicast.
 */
export const NSD_PERMISSIONS = ["ACCESS_NETWORK_STATE", "CHANGE_WIFI_MULTICAST_STATE"];

export function checkNsd(manifest, source) {
  const errors = NSD_PERMISSIONS.filter(
    (p) =>
      !new RegExp(String.raw`<uses-permission\b[^>]*android:name="android\.permission\.${p}"`).test(
        manifest,
      ),
  ).map((p) => `manifest: permission ${p} is missing`);
  if (source === undefined) return [...errors, "nsd: NsdBridge.kt is missing"];
  if (!/"_arena360-hub\._tcp"/.test(source)) {
    errors.push("nsd: the bridge must advertise the _arena360-hub._tcp service type");
  }
  if (!/\bNsdManager\b/.test(source) || !/\bregisterService\s*\(/.test(source)) {
    errors.push("nsd: the bridge must register the service through NsdManager");
  }
  if (!/\bregisterDefaultNetworkCallback\s*\(/.test(source)) {
    errors.push("nsd: the bridge must watch the default network to re-register on an IP change");
  }
  return errors;
}

/**
 * Bluetooth printing lists printers already paired in Android settings and prints over RFCOMM: it
 * asks for BLUETOOTH_CONNECT (Android 12+) and the install-time BLUETOOTH up to Android 11, and
 * never for scanning, advertising, admin or location, so the Hub cannot pair or track anything.
 */
export const BLUETOOTH_FORBIDDEN = [
  "BLUETOOTH_SCAN",
  "BLUETOOTH_ADVERTISE",
  "BLUETOOTH_ADMIN",
  "ACCESS_FINE_LOCATION",
  "ACCESS_COARSE_LOCATION",
];

export function checkBluetooth(manifest, source) {
  const errors = [];
  const permission = (p) =>
    manifest.match(
      new RegExp(String.raw`<uses-permission\b[^>]*android:name="android\.permission\.${p}"[^>]*>`),
    )?.[0];
  if (permission("BLUETOOTH_CONNECT") === undefined) {
    errors.push("manifest: permission BLUETOOTH_CONNECT is missing");
  }
  const legacy = permission("BLUETOOTH");
  if (legacy === undefined) errors.push("manifest: permission BLUETOOTH is missing");
  else if (attr(legacy, "android:maxSdkVersion") !== "30") {
    errors.push('manifest: permission BLUETOOTH must have maxSdkVersion="30"');
  }
  for (const p of BLUETOOTH_FORBIDDEN) {
    if (permission(p) !== undefined) errors.push(`manifest: permission ${p} must not be requested`);
  }
  const feature = manifest.match(
    /<uses-feature\b[^>]*android:name="android\.hardware\.bluetooth"[^>]*>/,
  )?.[0];
  if (feature === undefined || attr(feature, "android:required") !== "false") {
    errors.push(
      'manifest: <uses-feature android.hardware.bluetooth> must be declared with required="false"',
    );
  }
  if (source === undefined) return [...errors, "bluetooth: BluetoothBridge.kt is missing"];
  if (!/\bcreateRfcommSocketToServiceRecord\s*\(/.test(source)) {
    errors.push("bluetooth: the bridge must print over an RFCOMM (SPP) socket");
  }
  if (/\bstartDiscovery\s*\(|\bcreateBond\s*\(/.test(source)) {
    errors.push("bluetooth: the bridge must not scan or pair, only use paired printers");
  }
  return errors;
}

/**
 * The activity handles rotation, split-screen and a system font-size change itself (it is not
 * recreated, so the open screen, sheet and typed text survive): `configChanges` must keep these.
 */
export const ACTIVITY_CONFIG_CHANGES = ["orientation", "screenSize", "screenLayout", "fontScale"];

export function checkActivity(manifest) {
  const activity = element(manifest, "activity", ".MainActivity");
  if (activity === undefined) return ["manifest: .MainActivity is not declared"];
  const changes = (attr(activity, "android:configChanges") ?? "").split("|");
  return ACTIVITY_CONFIG_CHANGES.filter((c) => !changes.includes(c)).map(
    (c) => `manifest: .MainActivity configChanges must include ${c}`,
  );
}

export function checkRepoManifest(root) {
  const main = join(root, ANDROID_DIR);
  const res = {};
  const xmlDir = join(main, "res", "xml");
  if (existsSync(xmlDir)) {
    for (const f of readdirSync(xmlDir)) {
      if (f.endsWith(".xml")) res[f.slice(0, -4)] = readFileSync(join(xmlDir, f), "utf8");
    }
  }
  const path = join(main, "AndroidManifest.xml");
  if (!existsSync(path)) return [`${path} is missing`];
  const manifest = readFileSync(path, "utf8");
  const bridge = join(main, "java", "in", "arena360", "hub", "KeystoreBridge.kt");
  const nsd = join(main, "java", "in", "arena360", "hub", "NsdBridge.kt");
  const bt = join(main, "java", "in", "arena360", "hub", "BluetoothBridge.kt");
  return [
    ...checkManifest(manifest, res),
    ...checkService(manifest),
    ...checkDuplicates(manifest),
    ...checkActivity(manifest),
    ...checkKeystore(existsSync(bridge) ? readFileSync(bridge, "utf8") : undefined),
    ...checkNsd(manifest, existsSync(nsd) ? readFileSync(nsd, "utf8") : undefined),
    ...checkBluetooth(manifest, existsSync(bt) ? readFileSync(bt, "utf8") : undefined),
  ];
}

export function elfProblem(buf) {
  if (buf.length < 64 || buf.readUInt32BE(0) !== 0x7f454c46) return "not an ELF file";
  if (buf[4] !== 2 || buf[5] !== 1) return "not a 64-bit little-endian ELF";
  const phoff = Number(buf.readBigUInt64LE(0x20));
  const phentsize = buf.readUInt16LE(0x36);
  const phnum = buf.readUInt16LE(0x38);
  if (phentsize < 56 || phoff + phentsize * phnum > buf.length) return "truncated program headers";
  let loads = 0;
  for (let i = 0; i < phnum; i++) {
    const at = phoff + i * phentsize;
    if (buf.readUInt32LE(at) !== 1) continue;
    loads++;
    const align = buf.readBigUInt64LE(at + 0x30);
    if (align % PAGE_ALIGN !== 0n) return `LOAD segment aligned to ${align}, not 16 KB`;
  }
  return loads === 0 ? "no LOAD segments" : null;
}

function* walk(dir) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) yield* walk(p);
    else yield p;
  }
}

export function checkAab(dir) {
  const lib = join(dir, "base", "lib");
  if (!existsSync(lib)) return ["base/lib is missing: the AAB has no native libraries"];
  const errors = [];
  const found = new Set();
  for (const abi of readdirSync(lib)) {
    for (const file of walk(join(lib, abi))) {
      if (!file.endsWith(".so")) continue;
      found.add(abi);
      const problem = elfProblem(readFileSync(file));
      if (problem) errors.push(`base/lib/${abi}/${file.split(/[\\/]/).pop()}: ${problem}`);
    }
  }
  for (const abi of REQUIRED_ABIS) {
    if (!found.has(abi)) errors.push(`ABI ${abi} has no native library`);
  }
  return errors;
}

function main() {
  const [mode, dir] = process.argv.slice(2);
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  let errors;
  if (mode === "manifest") errors = checkRepoManifest(root);
  else if (mode === "aab" && dir) errors = checkAab(dir);
  else {
    console.error("usage: check-android.mjs manifest | aab <extracted-aab-dir>");
    process.exit(64);
  }
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log(`android ${mode} check: ok`);
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

if (isMain) {
  main();
}
