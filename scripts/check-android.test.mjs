import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { checkAab, checkManifest, checkRepoManifest, elfProblem } from "./check-android.mjs";

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
