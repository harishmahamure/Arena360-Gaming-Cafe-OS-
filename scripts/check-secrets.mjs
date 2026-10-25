#!/usr/bin/env node
// Plaintext-secret check: fails on committed secrets or env files. Reads the git file list and
// the files themselves (no network). Rules:
//   - a file under deploy/secrets/ (other than README.md) without SOPS metadata (`sops.mac`);
//   - any `.env` / `.env.*` / `*.env` file (`.env.example` excepted);
//   - `kind: Secret` anywhere under deploy/k8s/ (Secrets come from SOPS at deploy time);
//   - an age private key, a PEM private key or a JWT-shaped token over 100 characters in any
//     text file outside test fixtures (`fixtures/` or `testdata/` directories).

import { execFileSync } from "node:child_process";
import { readFileSync, realpathSync, statSync } from "node:fs";
import { dirname, join, posix } from "node:path";
import { fileURLToPath } from "node:url";
import { parseAllDocuments } from "yaml";

// Split so this file never matches its own rules.
// An age identity: the prefix plus 58 bech32 characters (the bare prefix in prose passes).
const AGE_KEY = new RegExp("AGE-SECRET-" + "KEY-1[QPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L]{58}");
const PEM_KEY = new RegExp("-----BEGIN (?:[A-Z0-9]+ )*PRIVATE " + "KEY-----");
const JWT = /\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/g;
const JWT_MIN = 101;
const K8S_SECRET = /^\s*kind:\s*["']?Secret["']?\s*(?:#.*)?$/m;
const MAX_BYTES = 4 * 1024 * 1024;

/** Is `path` a test fixture (exempt from the content rules)? */
export function isFixture(path) {
  return path.split("/").some((seg) => seg === "fixtures" || seg === "testdata");
}

/** Is `path` an env file? */
export function isEnvFile(path) {
  const base = posix.basename(path);
  if (base === ".env.example") return false;
  return base === ".env" || base.startsWith(".env.") || base.endsWith(".env");
}

/** Is `path` a signing keystore (the Android upload key must come from SOPS)? */
export function isKeystoreFile(path) {
  return /\.(jks|keystore|p12|pfx)$/i.test(path) || /(^|\/)(keystore|key)\.properties$/.test(path);
}

const ENC = /^ENC\[[\s\S]*\]$/;

/** Every leaf under `node` that is not an `ENC[...]` string, as dotted paths. */
function plaintextLeaves(node, path) {
  if (node === null || node === undefined) return [];
  if (Array.isArray(node)) return node.flatMap((v, i) => plaintextLeaves(v, `${path}[${i}]`));
  if (typeof node === "object") {
    return Object.entries(node).flatMap(([k, v]) => plaintextLeaves(v, `${path}.${k}`));
  }
  return typeof node === "string" && ENC.test(node) ? [] : [path];
}

/**
 * Why `text` is not a fully SOPS-encrypted file: every YAML/JSON document must carry
 * `sops.mac` and every leaf under `data`/`stringData` must be `ENC[...]` (a hand-edited
 * plaintext value fails). Flat (dotenv/ini) SOPS files need `sops_mac` and only `ENC[...]`
 * values. Empty when encrypted.
 * @returns {string[]}
 */
export function sopsProblems(text) {
  let docs = null;
  try {
    docs = parseAllDocuments(text).map((d) => {
      if (d.errors.length > 0) throw d.errors[0];
      return d.toJS();
    });
  } catch {
    docs = null; // not YAML/JSON: the flat formats below
  }
  if (
    docs &&
    docs.length > 0 &&
    docs.every((d) => d && typeof d === "object" && !Array.isArray(d))
  ) {
    const problems = [];
    docs.forEach((doc, i) => {
      const where = docs.length > 1 ? `document ${i + 1} ` : "";
      if (!(typeof doc.sops?.mac === "string" && doc.sops.mac)) {
        problems.push(`${where}has no sops.mac`);
        return; // not encrypted at all: its values are plaintext by definition
      }
      for (const key of ["data", "stringData"]) {
        for (const leaf of plaintextLeaves(doc[key], key)) {
          problems.push(`${where}plaintext value at ${leaf}`);
        }
      }
    });
    return problems;
  }
  const lines = text.split(/\r?\n/).filter((l) => l.trim() && !/^\s*[#;]/.test(l));
  if (!lines.some((l) => /^sops_mac\s*=\s*\S+/.test(l))) return ["has no sops.mac"];
  return lines
    .filter((l) => !/^sops_/.test(l))
    .map((l) => l.split("="))
    .filter(([, ...v]) => !ENC.test(v.join("=").trim()))
    .map(([k]) => `plaintext value at ${k.trim()}`);
}

/** Does `text` carry SOPS metadata with a MAC and only encrypted values? */
export function hasSopsMac(text) {
  return sopsProblems(text).length === 0;
}

/**
 * Checks one file.
 * @param {string} path repository-relative, `/`-separated
 * @param {string} text file contents (UTF-8)
 * @returns {string[]} error messages
 */
export function checkFile(path, text) {
  const errors = [];
  if (isEnvFile(path)) errors.push(`${path}: env file committed`);
  if (isKeystoreFile(path)) errors.push(`${path}: signing keystore committed`);
  if (path.startsWith("deploy/secrets/") && posix.basename(path) !== "README.md") {
    for (const p of sopsProblems(text)) {
      errors.push(
        p === "has no sops.mac"
          ? `${path}: not SOPS-encrypted (no sops.mac)`
          : `${path}: not SOPS-encrypted (${p})`,
      );
    }
  }
  if (path.startsWith("deploy/k8s/") && K8S_SECRET.test(text)) {
    errors.push(`${path}: kind: Secret committed (Secrets come from deploy/secrets via SOPS)`);
  }
  if (!isFixture(path)) {
    if (AGE_KEY.test(text)) errors.push(`${path}: age private key`);
    if (PEM_KEY.test(text)) errors.push(`${path}: PEM private key`);
    for (const m of text.matchAll(JWT)) {
      if (m[0].length >= JWT_MIN) {
        errors.push(`${path}: JWT-shaped token (${m[0].length} characters)`);
        break;
      }
    }
  }
  return errors;
}

/** Tracked plus untracked-but-not-ignored files, relative to `root`. */
export function listFiles(root) {
  const out = execFileSync(
    "git",
    ["ls-files", "-z", "--cached", "--others", "--exclude-standard"],
    { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  return [...new Set(out.split("\0").filter(Boolean))].sort();
}

export function checkRepo(root, files = listFiles(root)) {
  const errors = [];
  for (const path of files) {
    let buf;
    try {
      const st = statSync(join(root, path));
      if (!st.isFile()) continue;
      if (st.size > MAX_BYTES) {
        if (isEnvFile(path)) errors.push(`${path}: env file committed`);
        continue;
      }
      buf = readFileSync(join(root, path));
    } catch {
      continue; // deleted in the working tree
    }
    if (buf.includes(0)) {
      if (isEnvFile(path)) errors.push(`${path}: env file committed`);
      if (path.startsWith("deploy/secrets/") && posix.basename(path) !== "README.md") {
        errors.push(`${path}: not SOPS-encrypted (no sops.mac)`);
      }
      continue;
    }
    errors.push(...checkFile(path, buf.toString("utf8")));
  }
  return errors;
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const errors = checkRepo(root);
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("secrets check: ok");
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
