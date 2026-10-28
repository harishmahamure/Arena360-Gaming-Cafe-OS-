import { test } from "node:test";
import assert from "node:assert/strict";
import { generateKeyPairSync, createVerify } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  ADS_GRADLE,
  AD_SDK_DATA_TYPES,
  DEVICE_SPEC,
  GRADLE,
  MANIFEST,
  Play,
  accessToken,
  checkListing,
  checkPromotion,
  checkRollout,
  decideHalt,
  readBuild,
} from "./play-release.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");
const listing = () => JSON.parse(read("deploy/play/listing.json"));
const rollout = () => JSON.parse(read("deploy/play/rollout.json"));
const build = () =>
  readBuild({
    gradle: read(GRADLE),
    manifest: read(MANIFEST),
    adsGradle: read(ADS_GRADLE),
    deviceSpec: read(DEVICE_SPEC),
  });

test("the committed listing and rollout pass against the real build", () => {
  assert.deepEqual(checkListing(listing(), build()), []);
  assert.deepEqual(checkRollout(rollout()), []);
});

test("the build facts are read from the sources", () => {
  assert.deepEqual(build(), {
    minSdk: 28,
    adSdkVersion: "25.5.0",
    serviceType: "connectedDevice",
    minRamGb: 3,
  });
});

test("the listing must say Contains ads, and match the ad SDK pin", () => {
  const l = listing();
  l.contains_ads = false;
  l.ad_sdk.version = "24.0.0";
  const errors = checkListing(l, build());
  assert.ok(errors.some((e) => e.includes("Contains ads")));
  assert.ok(errors.some((e) => e.includes("redo the data safety form")));
});

test("every data type the ad SDK collects must be declared as shared", () => {
  for (const type of AD_SDK_DATA_TYPES) {
    const l = listing();
    l.data_safety.collected = l.data_safety.collected.filter((c) => c.type !== type);
    assert.ok(
      checkListing(l, build()).some((e) => e.includes(type)),
      type,
    );
    const l2 = listing();
    l2.data_safety.collected.find((c) => c.type === type).shared = false;
    assert.ok(
      checkListing(l2, build()).some((e) => e.includes(`${type} as shared`)),
      type,
    );
  }
});

test("the foreground service type, RAM floor and minSdk must match the build", () => {
  const l = listing();
  l.foreground_service.types = ["dataSync"];
  l.device_catalogue.min_ram_mb = 2048;
  l.device_catalogue.min_sdk = 26;
  const errors = checkListing(l, build());
  assert.ok(errors.some((e) => e.includes("foreground service types")));
  assert.ok(errors.some((e) => e.includes("3072")));
  assert.ok(errors.some((e) => e.includes("min_sdk 26")));
  assert.ok(
    checkListing(listing(), { ...build(), minSdk: 26 }).some((e) =>
      e.includes("minSdk must be 28"),
    ),
  );
});

test("the rollout is 1, 10, 50, 100 over a week with positive thresholds", () => {
  const r = rollout();
  r.stages = r.stages.filter((s) => s.percent !== 10);
  r.halt_when.anr_rate_pct = 0;
  const errors = checkRollout(r);
  assert.ok(errors.some((e) => e.includes("1, 10, 50, 100")));
  assert.ok(errors.some((e) => e.includes("anr_rate_pct")));
  const slow = rollout();
  slow.stages[0].min_hours = 400;
  assert.ok(checkRollout(slow).some((e) => e.includes("stage holds")));
});

test("any rate above its threshold halts, one at the threshold does not", () => {
  const { halt_when } = rollout();
  assert.equal(decideHalt({ crash: 1.0, anr: 0.47, gap: 2.0 }, halt_when).halt, false);
  const d = decideHalt({ crash: 0.2, anr: 0.1, gap: 2.1 }, halt_when);
  assert.equal(d.halt, true);
  assert.match(d.reasons[0], /heartbeat-gap/);
  assert.throws(() => decideHalt({ crash: 0.2, anr: 0.1, gap: NaN }, halt_when), /no fleet/);
});

const CLEAN = { crashes: 0, accessibility_errors: 0, devices: 6 };
const NOW = "2026-11-02T12:00:00Z";

test("a rollout starts at 1% and only with a clean pre-launch report", () => {
  const r = rollout();
  const go = (percent, prelaunch) =>
    checkPromotion({ rollout: r, current: null, percent, now: NOW, prelaunch });
  assert.equal(go(1, CLEAN).ok, true);
  assert.equal(go(10, CLEAN).ok, false);
  for (const bad of [
    null,
    { ...CLEAN, crashes: 1 },
    { ...CLEAN, accessibility_errors: 2 },
    { ...CLEAN, devices: 0 },
  ]) {
    const v = go(1, bad);
    assert.equal(v.ok, false);
    assert.match(v.reason, /pre-launch/);
  }
});

test("stages go in order and each is held its minimum", () => {
  const r = rollout();
  const at = (percent, hoursAgo) => ({
    percent,
    since: new Date(Date.parse(NOW) - hoursAgo * 3_600_000).toISOString(),
  });
  const go = (current, percent) =>
    checkPromotion({ rollout: r, current, percent, now: NOW, prelaunch: CLEAN });
  assert.equal(go(at(1, 25), 10).ok, true);
  assert.match(go(at(1, 5), 10).reason, /hold it 24 h/);
  assert.match(go(at(1, 100), 50).reason, /next stage is 10%/);
  assert.equal(go(at(50, 80), 100).ok, true);
  assert.match(go(at(100, 500), 100).reason, /complete/);
  assert.match(go(at(1, 100), 7).reason, /not a stage/);
});

test("the access token is an RS256 JWT bearer grant signed with the account key", async () => {
  const { privateKey, publicKey } = generateKeyPairSync("rsa", {
    modulusLength: 2048,
    privateKeyEncoding: { type: "pkcs8", format: "pem" },
    publicKeyEncoding: { type: "spki", format: "pem" },
  });
  let sent;
  const fake = async (url, init) => {
    sent = { url, body: init.body };
    return { ok: true, json: async () => ({ access_token: "tok" }) };
  };
  const token = await accessToken(
    { client_email: "ci@x.iam", private_key: privateKey, token_uri: "https://token.test/t" },
    fake,
    1000,
  );
  assert.equal(token, "tok");
  assert.equal(sent.url, "https://token.test/t");
  const [h, c, s] = sent.body.get("assertion").split(".");
  assert.equal(JSON.parse(Buffer.from(h, "base64url")).alg, "RS256");
  const claims = JSON.parse(Buffer.from(c, "base64url"));
  assert.equal(claims.iss, "ci@x.iam");
  assert.equal(claims.scope, "https://www.googleapis.com/auth/androidpublisher");
  assert.equal(claims.exp, 4600);
  assert.ok(
    createVerify("RSA-SHA256").update(`${h}.${c}`).verify(publicKey, Buffer.from(s, "base64url")),
  );
});

function fakePlay() {
  const calls = [];
  const fetchFn = async (url, init) => {
    const path = url.replace(/^https:\/\/androidpublisher\.googleapis\.com/, "");
    calls.push({ method: init.method, path, body: init.body });
    const body = path.endsWith("/edits")
      ? { id: "E1" }
      : path.includes("/bundles")
        ? { versionCode: 42 }
        : {};
    return { ok: true, status: 200, text: async () => JSON.stringify(body) };
  };
  return { play: new Play("tok", "in.arena360.hub", fetchFn), calls };
}

const P = "/androidpublisher/v3/applications/in.arena360.hub";

test("an upload opens an edit, uploads the AAB, sets the internal track and commits", async () => {
  const { play, calls } = fakePlay();
  const code = await play.uploadInternal(Buffer.from("aab"), "notes");
  assert.equal(code, 42);
  assert.deepEqual(
    calls.map((c) => `${c.method} ${c.path}`),
    [
      `POST ${P}/edits`,
      `POST /upload${P}/edits/E1/bundles?uploadType=media`,
      `PUT ${P}/edits/E1/tracks/internal`,
      `POST ${P}/edits/E1:commit`,
    ],
  );
  const release = JSON.parse(calls[2].body).releases[0];
  assert.deepEqual(release.versionCodes, ["42"]);
  assert.equal(release.status, "completed");
});

test("a staged rollout sets userFraction, 100% completes, halt halts", async () => {
  const a = fakePlay();
  await a.play.setProduction(42, 10);
  assert.deepEqual(JSON.parse(a.calls[1].body).releases[0], {
    versionCodes: ["42"],
    status: "inProgress",
    userFraction: 0.1,
  });
  const b = fakePlay();
  await b.play.setProduction(42, 100);
  assert.equal(JSON.parse(b.calls[1].body).releases[0].status, "completed");
  const c = fakePlay();
  await c.play.halt(42, 0.5);
  assert.equal(JSON.parse(c.calls[1].body).releases[0].status, "halted");
});

test("an API error leaves the edit uncommitted", async () => {
  const calls = [];
  const play = new Play("tok", "p", async (url, init) => {
    calls.push(`${init.method} ${url}`);
    if (url.includes("/bundles")) return { ok: false, status: 403, text: async () => "denied" };
    return { ok: true, status: 200, text: async () => JSON.stringify({ id: "E1" }) };
  });
  await assert.rejects(play.uploadInternal(Buffer.from("x"), "n"), /403 denied/);
  assert.ok(!calls.some((c) => c.includes(":commit")));
});

test("the release workflow uploads only after the cloud gate, signed, to internal, never to production", async () => {
  const { parse } = await import("yaml");
  const wf = parse(read(".github/workflows/release-android.yml"));
  assert.deepEqual(wf.on.workflow_run.workflows, ["deploy-cloud"]);
  assert.equal(wf.jobs.upload.needs, "gate");
  const gate = wf.jobs.gate.steps.map((s) => s.run ?? "").join("\n");
  assert.match(gate, /play-release\.mjs cloud-live/);
  assert.match(gate, /play-release\.mjs check/);
  assert.match(wf.jobs.gate.if, /conclusion == 'success'/);
  const steps = wf.jobs.upload.steps.map((s) => s.run ?? "").join("\n");
  assert.match(steps, /never built unsigned/);
  assert.match(steps, /jarsigner -verify/);
  assert.match(steps, /play-release\.mjs upload/);
  assert.doesNotMatch(steps, /play-release\.mjs (promote|halt)/);
  assert.match(steps, /play-publisher\.enc\.yaml/);
  assert.doesNotMatch(
    read(".github/workflows/release-android.yml"),
    /service_account_json"?\s*:\s*\{/,
  );
});
