import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  checkAab,
  checkDuplicates,
  checkKeystore,
  checkManifest,
  checkNsd,
  checkRepoManifest,
  checkService,
  elfProblem,
} from "./check-android.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

function elf(aligns, { types = aligns.map(() => 1) } = {}) {
  const phoff = 64;
  const buf = Buffer.alloc(phoff + 56 * aligns.length);
  buf.writeUInt32BE(0x7f454c46, 0);
  buf[4] = 2;
  buf[5] = 1;
  buf.writeBigUInt64LE(BigInt(phoff), 0x20);
  buf.writeUInt16LE(56, 0x36);
  buf.writeUInt16LE(aligns.length, 0x38);
  aligns.forEach((a, i) => {
    buf.writeUInt32LE(types[i], phoff + 56 * i);
    buf.writeBigUInt64LE(BigInt(a), phoff + 56 * i + 0x30);
  });
  return buf;
}

test("the committed Android manifest passes", () => {
  assert.deepEqual(checkRepoManifest(ROOT), []);
});

const rules = `<exclude domain="root" path="." /><exclude domain="file" path="." /><exclude domain="database" path="." /><exclude domain="sharedpref" path="." /><exclude domain="external" path="." />`;
const res = {
  data: `<data-extraction-rules><cloud-backup>${rules}</cloud-backup><device-transfer>${rules}</device-transfer></data-extraction-rules>`,
  backup: `<full-backup-content>${rules}</full-backup-content>`,
  net: `<network-security-config><base-config cleartextTrafficPermitted="false" /><domain-config cleartextTrafficPermitted="true"><domain>127.0.0.1</domain></domain-config></network-security-config>`,
};
const manifest = (extra = {}) => {
  const a = {
    "android:allowBackup": "false",
    "android:dataExtractionRules": "@xml/data",
    "android:fullBackupContent": "@xml/backup",
    "android:networkSecurityConfig": "@xml/net",
    ...extra,
  };
  const attrs = Object.entries(a)
    .filter(([, v]) => v !== null)
    .map(([k, v]) => `${k}="${v}"`)
    .join(" ");
  return `<manifest><application ${attrs}></application></manifest>`;
};

test("a manifest with every backup exclusion passes", () => {
  assert.deepEqual(checkManifest(manifest(), res), []);
});

test("allowBackup true or unset fails", () => {
  assert.equal(checkManifest(manifest({ "android:allowBackup": "true" }), res).length, 1);
  assert.equal(checkManifest(manifest({ "android:allowBackup": null }), res).length, 1);
});

test("missing rule files and attributes fail", () => {
  assert.match(
    checkManifest(manifest({ "android:dataExtractionRules": null }), res)[0],
    /dataExtractionRules is not set/,
  );
  assert.match(
    checkManifest(manifest({ "android:fullBackupContent": "@xml/gone" }), res)[0],
    /missing gone\.xml/,
  );
});

test("a transfer section that leaves the database out fails", () => {
  const weak = {
    ...res,
    data: res.data.replace(
      `<device-transfer>${rules}</device-transfer>`,
      `<device-transfer><exclude domain="root" path="." /><exclude domain="file" path="." /><exclude domain="sharedpref" path="." /><exclude domain="external" path="." /></device-transfer>`,
    ),
  };
  assert.deepEqual(checkManifest(manifest(), weak), [
    "dataExtractionRules device-transfer: does not exclude the database domain",
  ]);
});

test("cleartext to anything but loopback fails", () => {
  const open = {
    ...res,
    net: res.net.replace("127.0.0.1", "cafe.example"),
  };
  assert.match(checkManifest(manifest(), open)[0], /cafe\.example/);
  const lax = {
    ...res,
    net: res.net.replace('<base-config cleartextTrafficPermitted="false" />', ""),
  };
  assert.match(checkManifest(manifest(), lax)[0], /base-config/);
});

test("ELF alignment: 16 KB and 64 KB pass, 4 KB fails, non-LOAD ignored", () => {
  assert.equal(elfProblem(elf([16384, 16384])), null);
  assert.equal(elfProblem(elf([65536])), null);
  assert.match(elfProblem(elf([16384, 4096])), /4096/);
  assert.equal(elfProblem(elf([4096, 16384], { types: [4, 1] })), null);
  assert.equal(elfProblem(elf([4096], { types: [4] })), "no LOAD segments");
  assert.equal(
    elfProblem(Buffer.from("MZ not elf, padded to be long enough ".repeat(3))),
    "not an ELF file",
  );
  assert.equal(elfProblem(elf([16384]).subarray(0, 100)), "truncated program headers");
});

function aab(libs) {
  const dir = mkdtempSync(join(tmpdir(), "aab-"));
  for (const [path, buf] of Object.entries(libs)) {
    mkdirSync(dirname(join(dir, path)), { recursive: true });
    writeFileSync(join(dir, path), buf);
  }
  return dir;
}

test("an AAB with both ABIs aligned to 16 KB passes", () => {
  const dir = aab({
    "base/lib/arm64-v8a/liba.so": elf([16384]),
    "base/lib/x86_64/liba.so": elf([16384]),
    "base/manifest/AndroidManifest.xml": Buffer.from("x"),
  });
  try {
    assert.deepEqual(checkAab(dir), []);
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test("an AAB with a 4 KB library, a missing ABI or no libraries fails", () => {
  const bad = aab({
    "base/lib/arm64-v8a/liba.so": elf([4096]),
    "base/lib/x86_64/liba.so": elf([16384]),
    "base/lib/armeabi-v7a/libb.so": Buffer.from("junk"),
  });
  const noX86 = aab({ "base/lib/arm64-v8a/liba.so": elf([16384]) });
  const none = aab({ "base/manifest/AndroidManifest.xml": Buffer.from("x") });
  try {
    assert.deepEqual(checkAab(bad), [
      "base/lib/arm64-v8a/liba.so: LOAD segment aligned to 4096, not 16 KB",
      "base/lib/armeabi-v7a/libb.so: not an ELF file",
    ]);
    assert.deepEqual(checkAab(noX86), ["ABI x86_64 has no native library"]);
    assert.match(checkAab(none)[0], /no native libraries/);
  } finally {
    for (const d of [bad, noX86, none]) rmSync(d, { recursive: true });
  }
});

test("attribute order in an exclude does not matter, a narrower path does", () => {
  const swapped = {
    ...res,
    backup: res.backup.replaceAll(
      /<exclude domain="(\w+)" path="\." \/>/g,
      '<exclude path="." domain="$1"/>',
    ),
  };
  assert.deepEqual(checkManifest(manifest(), swapped), []);
  const narrow = {
    ...res,
    backup: res.backup.replace(
      '<exclude domain="file" path="." />',
      '<exclude domain="file" path="db" />',
    ),
  };
  assert.deepEqual(checkManifest(manifest(), narrow), [
    "fullBackupContent: does not exclude the file domain",
  ]);
});

const serviceManifest = (drop = []) =>
  [
    ...[
      "FOREGROUND_SERVICE",
      "FOREGROUND_SERVICE_CONNECTED_DEVICE",
      "WAKE_LOCK",
      "POST_NOTIFICATIONS",
      "RECEIVE_BOOT_COMPLETED",
      "REQUEST_IGNORE_BATTERY_OPTIMIZATIONS",
    ]
      .filter((p) => !drop.includes(p))
      .map((p) => `<uses-permission android:name="android.permission.${p}" />`),
    drop.includes("service")
      ? ""
      : `<service android:name=".HubService" android:exported="${drop.includes("exported") ? "true" : "false"}" android:foregroundServiceType="${drop.includes("type") ? "dataSync" : "connectedDevice"}" />`,
    drop.includes("receiver")
      ? ""
      : `<receiver android:name=".BootReceiver" android:exported="true"><intent-filter><action android:name="android.intent.action.BOOT_COMPLETED" />${drop.includes("replaced") ? "" : '<action android:name="android.intent.action.MY_PACKAGE_REPLACED" />'}</intent-filter></receiver>`,
  ].join("");

test("a manifest with the foreground service, its permissions and the boot receiver passes", () => {
  assert.deepEqual(checkService(serviceManifest()), []);
});

test("a missing permission, service, wrong type or exported service fails", () => {
  assert.deepEqual(checkService(serviceManifest(["WAKE_LOCK"])), [
    "manifest: permission WAKE_LOCK is missing",
  ]);
  assert.deepEqual(checkService(serviceManifest(["service"])), [
    "manifest: .HubService is not declared",
  ]);
  assert.match(checkService(serviceManifest(["type"]))[0], /connectedDevice/);
  assert.match(checkService(serviceManifest(["exported"]))[0], /exported="false"/);
});

test("the boot receiver must listen for boot and for a package update", () => {
  assert.deepEqual(checkService(serviceManifest(["receiver"])), [
    "manifest: .BootReceiver is not declared",
  ]);
  assert.deepEqual(checkService(serviceManifest(["replaced"])), [
    "manifest: .BootReceiver does not listen for MY_PACKAGE_REPLACED",
  ]);
});

test("a manifest that declares a permission, service or receiver twice fails", () => {
  const once = serviceManifest();
  assert.deepEqual(checkDuplicates(once), []);
  assert.deepEqual(checkDuplicates(once + once), [
    "manifest: <uses-permission android.permission.FOREGROUND_SERVICE> is declared more than once",
    "manifest: <uses-permission android.permission.FOREGROUND_SERVICE_CONNECTED_DEVICE> is declared more than once",
    "manifest: <uses-permission android.permission.WAKE_LOCK> is declared more than once",
    "manifest: <uses-permission android.permission.POST_NOTIFICATIONS> is declared more than once",
    "manifest: <uses-permission android.permission.RECEIVE_BOOT_COMPLETED> is declared more than once",
    "manifest: <uses-permission android.permission.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS> is declared more than once",
    "manifest: <service .HubService> is declared more than once",
    "manifest: <receiver .BootReceiver> is declared more than once",
  ]);
});

const bridge = `
  // The key never leaves the Keystore; never call getEncoded() on it.
  private const val PROVIDER = "AndroidKeyStore"
  private const val TRANSFORMATION = "AES/GCM/NoPadding"
  KeyGenParameterSpec.Builder(ALIAS, PURPOSES).build()
`;

test("a bridge that keeps an AES-GCM key in the AndroidKeyStore passes", () => {
  assert.deepEqual(checkKeystore(bridge), []);
});

test("a missing bridge, provider, mode or in-Keystore key generation fails", () => {
  assert.deepEqual(checkKeystore(undefined), ["keystore: KeystoreBridge.kt is missing"]);
  assert.deepEqual(checkKeystore(bridge.replace('"AndroidKeyStore"', '"BC"')), [
    "keystore: the bridge must use the AndroidKeyStore provider",
  ]);
  assert.deepEqual(checkKeystore(bridge.replace("AES/GCM/NoPadding", "AES/CBC/PKCS7Padding")), [
    "keystore: the bridge must use AES/GCM/NoPadding",
  ]);
  assert.deepEqual(checkKeystore(bridge.replace("KeyGenParameterSpec.Builder", "Other.Builder")), [
    "keystore: the key must be generated in the Keystore (KeyGenParameterSpec)",
  ]);
});

test("a bridge that exports, imports or loosens the key fails", () => {
  for (const [code, what] of [
    ["val raw = key.encoded", "reads the key bytes"],
    ["val raw = key.getEncoded()", "reads the key bytes"],
    ['SecretKeySpec(bytes, "AES")', "builds a key from raw bytes"],
    [".setRandomizedEncryptionRequired(false)", "turns off randomised encryption"],
    [".setUserAuthenticationRequired(false)", "loosens user authentication"],
    ["store.setEntry(ALIAS, entry, null)", "imports a key"],
  ]) {
    const errors = checkKeystore(bridge + code);
    assert.equal(errors.length, 1, code);
    assert.match(errors[0], new RegExp(what), code);
  }
});

test("the committed bridge keeps the key in the Keystore", () => {
  assert.deepEqual(
    checkRepoManifest(ROOT).filter((e) => e.startsWith("keystore:")),
    [],
  );
});

const NSD_MANIFEST = [
  '<uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />',
  '<uses-permission android:name="android.permission.CHANGE_WIFI_MULTICAST_STATE" />',
].join("\n");
const NSD_BRIDGE = `
  private const val SERVICE_TYPE = "_arena360-hub._tcp"
  nsd.registerService(info, NsdManager.PROTOCOL_DNS_SD, listener)
  connectivity()?.registerDefaultNetworkCallback(cb)
`;

test("a bridge that registers the Arena service and watches the network passes", () => {
  assert.deepEqual(checkNsd(NSD_MANIFEST, NSD_BRIDGE), []);
});

test("a missing bridge, service type, registration, network callback or permission fails", () => {
  assert.deepEqual(checkNsd(NSD_MANIFEST, undefined), ["nsd: NsdBridge.kt is missing"]);
  assert.deepEqual(checkNsd(NSD_MANIFEST, NSD_BRIDGE.replace("_arena360-hub", "_other")), [
    "nsd: the bridge must advertise the _arena360-hub._tcp service type",
  ]);
  assert.deepEqual(checkNsd(NSD_MANIFEST, NSD_BRIDGE.replace("registerService", "noop")), [
    "nsd: the bridge must register the service through NsdManager",
  ]);
  assert.deepEqual(
    checkNsd(NSD_MANIFEST, NSD_BRIDGE.replace("registerDefaultNetworkCallback", "x")),
    ["nsd: the bridge must watch the default network to re-register on an IP change"],
  );
  assert.deepEqual(checkNsd(NSD_MANIFEST.replace("ACCESS_NETWORK_STATE", "X"), NSD_BRIDGE), [
    "manifest: permission ACCESS_NETWORK_STATE is missing",
  ]);
});

test("the committed bridge and manifest satisfy the NsdManager rules", () => {
  assert.deepEqual(
    checkRepoManifest(ROOT).filter((e) => e.startsWith("nsd:")),
    [],
  );
});
