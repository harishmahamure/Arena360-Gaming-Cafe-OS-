import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { addRelease, main, releaseLine, sha256File } from "./release-manifest.mjs";
import { parseManifest } from "../apps/marketing/src/lib/manifest.ts";

const SIG = "https://downloads.arena360.cloud/releases/hub-windows/1.0.0/Hub.exe.sig";

function withFile(body, run) {
  const dir = mkdtempSync(join(tmpdir(), "release-manifest-"));
  try {
    const path = join(dir, "Arena360-Hub-1.0.0-x64.exe");
    writeFileSync(path, body);
    return run(path, dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("a line carries the installer's real size and SHA-256", () => {
  withFile("installer bytes", (path) => {
    const line = releaseLine({
      product: "hub-windows",
      version: "1.0.0",
      path,
      signatureUrl: SIG,
      publishedAt: "2026-11-01T00:00:00Z",
    });
    assert.equal(line.channel, "stable");
    assert.equal(line.file, "Arena360-Hub-1.0.0-x64.exe");
    assert.equal(line.size, 15);
    assert.equal(line.sha256, sha256File(path));
    assert.match(line.sha256, /^[0-9a-f]{64}$/);
    assert.deepEqual(parseManifest({ releases: [line] }), [line]); // the Downloads page accepts it
  });
});

test("a line the Downloads page would drop is refused", () => {
  withFile("x", (path) => {
    const base = { product: "hub-windows", version: "1.0.0", path, signatureUrl: SIG };
    assert.throws(() => releaseLine({ ...base, product: "hub-android" }), /unknown product/);
    assert.throws(() => releaseLine({ ...base, version: "../1" }), /bad version/);
    assert.throws(() => releaseLine({ ...base, signatureUrl: "http://x/y.sig" }), /https/);
    assert.throws(() => releaseLine({ ...base, publishedAt: "yesterday" }), /published_at/);
  });
  withFile("", (path) => {
    assert.throws(
      () => releaseLine({ product: "pi-image", version: "1", path, signatureUrl: SIG }),
      /empty/,
    );
  });
});

test("adding a release keeps other products and replaces the same version", () => {
  const a = {
    product: "hub-windows",
    channel: "stable",
    version: "1.0.0",
    published_at: "2026-10-01T00:00:00Z",
    sha256: "a",
  };
  const b = {
    product: "kiosk-windows",
    channel: "stable",
    version: "1.0.0",
    published_at: "2026-10-01T00:00:00Z",
    sha256: "b",
  };
  const a2 = { ...a, sha256: "a2" };
  const a3 = { ...a, version: "1.1.0", published_at: "2026-11-01T00:00:00Z", sha256: "a3" };
  const first = addRelease(addRelease(null, a), b);
  assert.deepEqual(first.releases, [a, b]);
  assert.deepEqual(addRelease(first, a2).releases, [a2, b]);
  assert.deepEqual(addRelease(first, a3).releases, [a3, a, b]);
});

test("the add command writes a manifest the Downloads page reads", () => {
  withFile("installer bytes", (path, dir) => {
    const manifest = join(dir, "manifest.json");
    const run = (version, at) =>
      main([
        "add",
        "--manifest",
        manifest,
        "--product",
        "hub-windows",
        "--version",
        version,
        "--file",
        path,
        "--signature-url",
        SIG,
        "--published-at",
        at,
      ]);
    const quiet = console.log;
    console.log = () => {};
    try {
      assert.equal(run("1.0.0", "2026-11-01T00:00:00Z"), 0);
      assert.equal(run("1.0.1", "2026-11-02T00:00:00Z"), 0);
    } finally {
      console.log = quiet;
    }
    const releases = parseManifest(JSON.parse(readFileSync(manifest, "utf8")));
    assert.equal(releases.length, 1); // the page lists one row per product
    assert.equal(releases[0].version, "1.0.1");
  });
});

test("the Downloads page drops lines it cannot show a checksum for and keeps the newest per product", () => {
  const good = {
    product: "kiosk-windows",
    channel: "stable",
    version: "2.0.0",
    file: "Kiosk.exe",
    size: 10,
    sha256: "c".repeat(64),
    signature_url: "https://downloads.arena360.cloud/k.sig",
    published_at: "2026-11-01T00:00:00Z",
  };
  const older = { ...good, version: "1.0.0", published_at: "2026-10-01T00:00:00Z" };
  const lines = [
    older,
    good,
    { ...good, product: "hub-windows", sha256: "" },
    { ...good, product: "pi-image", channel: "beta" },
    { ...good, product: "hub-android" },
    { ...good, product: "hub-windows", file: "../evil.exe" },
    "nonsense",
  ];
  assert.deepEqual(parseManifest({ releases: lines }), [good]);
  assert.throws(() => parseManifest({}), /not a release manifest/);
  assert.throws(() => parseManifest(null), /not a release manifest/);
});
