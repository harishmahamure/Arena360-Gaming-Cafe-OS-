import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  checkFile,
  checkRepo,
  hasSopsMac,
  isEnvFile,
  isKeystoreFile,
  sopsProblems,
} from "./check-secrets.mjs";

// Fixtures are assembled at run time so this file never matches the rules itself.
const AGE = "AGE-SECRET-" + "KEY-1QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ";
const PEM = "-----BEGIN EC PRIVATE " + "KEY-----\nMHcCAQEE\n-----END EC PRIVATE KEY-----";
const JWT = (n) => "eyJ" + "a".repeat(n) + ".eyJ" + "b".repeat(n) + "." + "c".repeat(n);

const SOPS_YAML = `apiVersion: v1
kind: Secret
metadata:
  name: arena-cloud-secrets
stringData:
  ARENA_CONTROL_DB_TOKEN: ENC[AES256_GCM,data:abc,iv:x,tag:y,type:str]
sops:
  age:
    - recipient: age1xyz
      enc: |
        -----BEGIN AGE ENCRYPTED FILE-----
        abc
        -----END AGE ENCRYPTED FILE-----
  lastmodified: "2026-10-24T00:00:00Z"
  mac: ENC[AES256_GCM,data:mac,iv:x,tag:y,type:str]
  version: 3.13.3
`;

test("an unencrypted file under deploy/secrets fails naming the file", () => {
  assert.deepEqual(
    checkFile(
      "deploy/secrets/x.yaml",
      "apiVersion: v1\nkind: Secret\nstringData:\n  ARENA_CONTROL_DB_TOKEN: plain\n",
    ),
    ["deploy/secrets/x.yaml: not SOPS-encrypted (no sops.mac)"],
  );
  // sops metadata without a MAC is not enough
  assert.equal(hasSopsMac("a: 1\nsops:\n  version: 3.13.3\n"), false);
});

test("a SOPS-encrypted file and the README under deploy/secrets pass", () => {
  assert.deepEqual(checkFile("deploy/secrets/staging.enc.yaml", SOPS_YAML), []);
  assert.deepEqual(checkFile("deploy/secrets/README.md", "# how to encrypt\n"), []);
  assert.equal(hasSopsMac('{"a":"ENC[x]","sops":{"mac":"ENC[y]"}}'), true);
  assert.equal(hasSopsMac("A=ENC[x]\nsops_mac=ENC[y]\n"), true);
});

test("env files fail, .env.example passes", () => {
  for (const p of [".env", "apps/owner/.env", ".env.production", "deploy/prod.env"]) {
    assert.equal(isEnvFile(p), true, p);
    assert.deepEqual(checkFile(p, "X=1\n"), [`${p}: env file committed`], p);
  }
  for (const p of [".env.example", "apps/owner/.env.example", "src/env.rs", "environment.md"]) {
    assert.equal(isEnvFile(p), false, p);
  }
});

test("kind: Secret under deploy/k8s fails", () => {
  for (const kind of ["kind: Secret", 'kind: "Secret"', "  kind: Secret # nope"]) {
    assert.deepEqual(checkFile("deploy/k8s/base/s.yaml", `apiVersion: v1\n${kind}\n`), [
      "deploy/k8s/base/s.yaml: kind: Secret committed (Secrets come from deploy/secrets via SOPS)",
    ]);
  }
  assert.deepEqual(checkFile("deploy/k8s/base/d.yaml", "kind: Deployment\n# kind: SecretX\n"), []);
  assert.deepEqual(checkFile("deploy/k8s/base/d.yaml", "kind: SealedSecretish\n"), []);
});

test("age keys, PEM private keys and long JWTs fail outside fixtures", () => {
  assert.deepEqual(checkFile("notes.md", `key ${AGE}`), ["notes.md: age private key"]);
  // the bare prefix (in prose) is not a key
  assert.deepEqual(checkFile("notes.md", "AGE-SECRET-" + "KEY-1 lines"), []);
  assert.deepEqual(checkFile("a/b.rs", PEM), ["a/b.rs: PEM private key"]);
  const long = JWT(40);
  assert.ok(long.length > 100);
  assert.deepEqual(checkFile("c.ts", `const t = "${long}";`), [
    `c.ts: JWT-shaped token (${long.length} characters)`,
  ]);
  // short JWT-shaped strings pass
  assert.deepEqual(checkFile("c.ts", `const t = "${JWT(10)}";`), []);
  // a PEM certificate or public key is fine
  assert.deepEqual(checkFile("c.pem", "-----BEGIN CERTIFICATE-----\nabc\n"), []);
});

test("test fixtures are exempt from the content rules", () => {
  for (const p of ["scripts/fixtures/k.txt", "crates/x/tests/testdata/k.pem"]) {
    assert.deepEqual(checkFile(p, `${AGE}\n${PEM}\n${JWT(40)}`), [], p);
  }
});

test("checkRepo reads tracked and untracked files, skipping ignored ones", () => {
  const dir = mkdtempSync(join(tmpdir(), "check-secrets-"));
  try {
    execFileSync("git", ["init", "-q"], { cwd: dir });
    mkdirSync(join(dir, "deploy/secrets"), { recursive: true });
    mkdirSync(join(dir, "deploy/k8s"), { recursive: true });
    writeFileSync(join(dir, ".gitignore"), "ignored.txt\n");
    writeFileSync(join(dir, "ignored.txt"), AGE);
    writeFileSync(join(dir, "deploy/secrets/staging.enc.yaml"), SOPS_YAML);
    writeFileSync(join(dir, "deploy/secrets/x.yaml"), "a: plain\n");
    writeFileSync(join(dir, "deploy/k8s/s.yaml"), "kind: Secret\n");
    writeFileSync(join(dir, "prod.env"), "A=1\n");
    assert.deepEqual(checkRepo(dir), [
      "deploy/k8s/s.yaml: kind: Secret committed (Secrets come from deploy/secrets via SOPS)",
      "deploy/secrets/x.yaml: not SOPS-encrypted (no sops.mac)",
      "prod.env: env file committed",
    ]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the repository passes", () => {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  assert.deepEqual(checkRepo(root), []);
});

const TLS_DOC = `apiVersion: v1
kind: Secret
metadata:
  name: origin-tls
type: kubernetes.io/tls
stringData:
  tls.crt: ENC[AES256_GCM,data:crt,iv:x,tag:y,type:str]
  tls.key: ENC[AES256_GCM,data:key,iv:x,tag:y,type:str]
sops:
  mac: ENC[AES256_GCM,data:mac2,iv:x,tag:y,type:str]
  version: 3.13.3
`;

test("a multi-document encrypted file passes; every document needs its own sops.mac", () => {
  assert.deepEqual(checkFile("deploy/secrets/staging.enc.yaml", `${SOPS_YAML}---\n${TLS_DOC}`), []);
  const noMac = TLS_DOC.replace(/  mac: .*\n/, "");
  assert.deepEqual(checkFile("deploy/secrets/staging.enc.yaml", `${SOPS_YAML}---\n${noMac}`), [
    "deploy/secrets/staging.enc.yaml: not SOPS-encrypted (document 2 has no sops.mac)",
  ]);
});

test("a hand-edited plaintext value inside an encrypted file fails", () => {
  const edited = SOPS_YAML.replace(
    "ARENA_CONTROL_DB_TOKEN: ENC[AES256_GCM,data:abc,iv:x,tag:y,type:str]",
    "ARENA_CONTROL_DB_TOKEN: plain-token",
  );
  assert.deepEqual(checkFile("deploy/secrets/staging.enc.yaml", edited), [
    "deploy/secrets/staging.enc.yaml: not SOPS-encrypted (plaintext value at stringData.ARENA_CONTROL_DB_TOKEN)",
  ]);
  const data = `${TLS_DOC}`
    .replace("stringData:", "data:")
    .replace(/tls\.key: ENC\[.*\]/, "tls.key: c2VjcmV0");
  assert.deepEqual(sopsProblems(data), ["plaintext value at data.tls.key"]);
  assert.deepEqual(sopsProblems("A=plain\nsops_mac=ENC[y]\n"), ["plaintext value at A"]);
});

test("a signing keystore is refused wherever it is", () => {
  for (const p of [
    "upload.jks",
    "apps/hub/src-tauri/gen/android/release.keystore",
    "a/B.P12",
    "k.pfx",
    "apps/hub/src-tauri/gen/android/keystore.properties",
  ]) {
    assert.equal(isKeystoreFile(p), true, p);
    assert.deepEqual(checkFile(p, ""), [`${p}: signing keystore committed`]);
  }
  for (const p of ["tauri.properties", "deploy/secrets/android-upload.enc.yaml", "jks.md"]) {
    assert.equal(isKeystoreFile(p), false, p);
  }
});
