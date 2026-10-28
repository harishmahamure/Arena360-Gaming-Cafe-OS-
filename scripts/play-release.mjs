#!/usr/bin/env node
// Google Play release tooling for the Android Hub (Story 4.16).
//
//   check                      the listing declarations and the rollout plan against the build
//                              (run by `pnpm check`; no network)
//   cloud-live <url>           exits 1 unless the cloud answers /healthz (cloud first)
//   upload <aab> --notes <t>   Play internal testing track (needs PLAY_SERVICE_ACCOUNT_JSON)
//   promote <versionCode> --percent <n> --prelaunch <file>
//                              starts or widens the production staged rollout; refuses a stage
//                              out of order, too early, or without a clean pre-launch report
//   halt <versionCode>         halts the production rollout
//   decide --crash <pct> --anr <pct> --gap <pct>
//                              prints whether the recorded thresholds say to halt
//
// Everything that decides something is a pure function exported for `play-release.test.mjs`.
// The Play Developer API client takes `fetch` so the tests run it against a fake.

import { createSign } from "node:crypto";
import { readFileSync, realpathSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");

export const GRADLE = "apps/hub/src-tauri/gen/android/app/build.gradle.kts";
export const MANIFEST = "apps/hub/src-tauri/gen/android/app/src/main/AndroidManifest.xml";
export const ADS_GRADLE =
  "apps/hub/src-tauri/plugins/tauri-plugin-arena-ads/android/build.gradle.kts";
export const DEVICE_SPEC = "crates/arena-proto/src/device_spec.rs";

/** The Data safety types the Google Mobile Ads SDK collects; the form must declare each as shared. */
export const AD_SDK_DATA_TYPES = [
  "device_or_other_ids",
  "approximate_location",
  "app_interactions",
  "crash_logs",
  "diagnostics",
];

// ---------------------------------------------------------------- listing and rollout checks

/** What the build declares, read from the Gradle files, the manifest and the device-spec floor. */
export function readBuild(files) {
  const num = (re, text) => {
    const m = re.exec(text);
    return m ? Number(m[1]) : null;
  };
  const service = /<service\b[^>]*android:name="\.HubService"[^>]*>/.exec(files.manifest)?.[0];
  return {
    minSdk: num(/\bminSdk\s*=\s*(\d+)/, files.gradle),
    adSdkVersion: /play-services-ads:([\w.-]+)"/.exec(files.adsGradle)?.[1] ?? null,
    serviceType: service
      ? (/android:foregroundServiceType="([^"]*)"/.exec(service)?.[1] ?? null)
      : null,
    minRamGb: num(/pub const MIN_RAM_GB: u32 = (\d+);/, files.deviceSpec),
  };
}

export function checkListing(listing, build) {
  const errors = [];
  if (listing.package_name !== "in.arena360.hub")
    errors.push("listing: package_name is not in.arena360.hub");
  if (listing.contains_ads !== true) errors.push('listing: must declare "Contains ads"');

  if (listing.ad_sdk?.version !== build.adSdkVersion) {
    errors.push(
      `listing: ad_sdk.version ${listing.ad_sdk?.version} is not the pinned SDK ${build.adSdkVersion}; redo the data safety form`,
    );
  }
  const collected = new Map((listing.data_safety?.collected ?? []).map((c) => [c.type, c]));
  for (const type of AD_SDK_DATA_TYPES) {
    const c = collected.get(type);
    if (c === undefined) errors.push(`listing: data safety does not declare ${type}`);
    else {
      if (c.shared !== true) errors.push(`listing: data safety must declare ${type} as shared`);
      if (!Array.isArray(c.purposes) || c.purposes.length === 0)
        errors.push(`listing: data safety ${type} has no purpose`);
    }
  }
  if (listing.data_safety?.encrypted_in_transit !== true)
    errors.push("listing: data safety must say data is encrypted in transit");

  const types = listing.foreground_service?.types ?? [];
  if (build.serviceType === null) errors.push("listing: .HubService has no foregroundServiceType");
  else if (types.length !== 1 || types[0] !== build.serviceType)
    errors.push(
      `listing: foreground service types ${JSON.stringify(types)} do not match the manifest (${build.serviceType})`,
    );
  if (!(listing.foreground_service?.declaration ?? "").trim())
    errors.push("listing: foreground service declaration text is empty");

  const cat = listing.device_catalogue ?? {};
  if (build.minRamGb === null) errors.push("listing: the device-spec RAM floor was not found");
  else if (cat.min_ram_mb !== build.minRamGb * 1024)
    errors.push(
      `listing: device_catalogue.min_ram_mb must be ${build.minRamGb * 1024} (3 GB floor)`,
    );
  if (cat.min_sdk !== build.minSdk)
    errors.push(
      `listing: device_catalogue.min_sdk ${cat.min_sdk} is not the Gradle minSdk ${build.minSdk}`,
    );
  if (build.minSdk === null || build.minSdk < 28)
    errors.push("build: minSdk must be 28 or more (Android 8.1 and earlier are excluded)");
  return errors;
}

export function checkRollout(rollout) {
  const errors = [];
  const stages = rollout.stages ?? [];
  const want = [1, 10, 50, 100];
  if (JSON.stringify(stages.map((s) => s.percent)) !== JSON.stringify(want))
    errors.push("rollout: stages must be 1, 10, 50, 100 percent in that order");
  const hours = stages.reduce((n, s) => n + (s.min_hours ?? 0), 0);
  if (rollout.total_days !== 7) errors.push("rollout: total_days must be 7");
  if (hours > rollout.total_days * 24 || hours < 4 * 24)
    errors.push(
      `rollout: the stage holds add up to ${hours} h; they must fill most of a 7-day week`,
    );
  for (const k of ["user_perceived_crash_rate_pct", "anr_rate_pct", "heartbeat_gap_hubs_pct"]) {
    const v = rollout.halt_when?.[k];
    if (typeof v !== "number" || !(v > 0))
      errors.push(`rollout: halt_when.${k} must be a positive number`);
  }
  return errors;
}

// ---------------------------------------------------------------- rollout decisions

/** The halt rule: any measured rate above its recorded threshold says halt. */
export function decideHalt(metrics, halt) {
  const rules = [
    ["crash", "user_perceived_crash_rate_pct", "Play user-perceived crash rate"],
    ["anr", "anr_rate_pct", "Play ANR rate"],
    ["gap", "heartbeat_gap_hubs_pct", "fleet heartbeat-gap share"],
  ];
  const reasons = [];
  for (const [m, k, label] of rules) {
    const v = metrics[m];
    if (typeof v !== "number" || Number.isNaN(v)) throw new Error(`no ${label} given`);
    if (v > halt[k]) reasons.push(`${label} ${v}% is above ${halt[k]}%`);
  }
  return { halt: reasons.length > 0, reasons };
}

/**
 * May the production rollout of `version` move to `percent`? `current` is
 * `{percent, since}` (ISO time) of the stage it is in now, or null before the first stage.
 */
export function checkPromotion({ rollout, current, percent, now, prelaunch }) {
  const percents = rollout.stages.map((s) => s.percent);
  const refuse = (reason) => ({ ok: false, reason });
  if (!percents.includes(percent))
    return refuse(`${percent}% is not a stage (${percents.join(", ")})`);
  const clean =
    prelaunch &&
    prelaunch.crashes === 0 &&
    prelaunch.accessibility_errors === 0 &&
    prelaunch.devices > 0;
  if (!clean)
    return refuse(
      "the Play pre-launch report is not recorded as clean (0 crashes, 0 accessibility errors)",
    );
  if (current === null) {
    return percent === percents[0] ? { ok: true } : refuse(`a rollout starts at ${percents[0]}%`);
  }
  const at = percents.indexOf(current.percent);
  if (percent !== percents[at + 1]) {
    return refuse(
      `from ${current.percent}% the next stage is ${percents[at + 1] ?? "none (it is complete)"}%`,
    );
  }
  const heldH = (Date.parse(now) - Date.parse(current.since)) / 3_600_000;
  const need = rollout.stages[at].min_hours;
  if (heldH < need)
    return refuse(`${current.percent}% has been live ${heldH.toFixed(1)} h; hold it ${need} h`);
  return { ok: true };
}

// ---------------------------------------------------------------- Play Developer API

const API = "https://androidpublisher.googleapis.com/androidpublisher/v3/applications";
const UPLOAD = "https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications";
const b64url = (b) => Buffer.from(b).toString("base64url");

/** A service-account access token (OAuth 2.0 JWT bearer grant, RS256). */
export async function accessToken(account, fetchFn = fetch, nowS = Math.floor(Date.now() / 1000)) {
  const head = b64url(JSON.stringify({ alg: "RS256", typ: "JWT" }));
  const claims = b64url(
    JSON.stringify({
      iss: account.client_email,
      scope: "https://www.googleapis.com/auth/androidpublisher",
      aud: account.token_uri ?? "https://oauth2.googleapis.com/token",
      iat: nowS,
      exp: nowS + 3600,
    }),
  );
  const sig = createSign("RSA-SHA256").update(`${head}.${claims}`).sign(account.private_key);
  const res = await fetchFn(account.token_uri ?? "https://oauth2.googleapis.com/token", {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
      assertion: `${head}.${claims}.${b64url(sig)}`,
    }),
  });
  if (!res.ok) throw new Error(`Google token request failed: ${res.status}`);
  return (await res.json()).access_token;
}

export class Play {
  constructor(token, pkg, fetchFn = fetch) {
    this.token = token;
    this.pkg = pkg;
    this.fetch = fetchFn;
  }

  async call(method, url, body, headers = {}) {
    const res = await this.fetch(url, {
      method,
      headers: { authorization: `Bearer ${this.token}`, ...headers },
      body,
    });
    const text = await res.text();
    if (!res.ok)
      throw new Error(`Play API ${method} ${url.replace(API, "")} failed: ${res.status} ${text}`);
    return text ? JSON.parse(text) : {};
  }

  json = (method, path, obj) =>
    this.call(
      method,
      `${API}/${this.pkg}/${path}`,
      obj === undefined ? undefined : JSON.stringify(obj),
      {
        "content-type": "application/json",
      },
    );

  /** One edit: opened, changed by `work(editId)` and committed; an error leaves it uncommitted. */
  async edit(work) {
    const { id } = await this.json("POST", "edits", {});
    const out = await work(id);
    await this.json("POST", `edits/${id}:commit`);
    return out;
  }

  /** Uploads the AAB and rolls it out to the whole internal testing track. */
  uploadInternal(aab, notes, track = "internal") {
    return this.edit(async (id) => {
      const up = await this.call(
        "POST",
        `${UPLOAD}/${this.pkg}/edits/${id}/bundles?uploadType=media`,
        aab,
        { "content-type": "application/octet-stream" },
      );
      await this.json("PUT", `edits/${id}/tracks/${track}`, {
        track,
        releases: [
          {
            versionCodes: [String(up.versionCode)],
            status: "completed",
            releaseNotes: [{ language: "en-US", text: notes }],
          },
        ],
      });
      return up.versionCode;
    });
  }

  /** The production release in progress, `{versionCodes, status, userFraction}` or null. */
  async productionRelease(track = "production") {
    return this.edit(async (id) => {
      const t = await this.json("GET", `edits/${id}/tracks/${track}`);
      return (
        (t.releases ?? []).find((r) => r.status === "inProgress" || r.status === "halted") ?? null
      );
    });
  }

  /** Sets the production release of `versionCode` to a staged `percent` (100 completes it). */
  setProduction(versionCode, percent, track = "production") {
    return this.edit(async (id) => {
      const release =
        percent >= 100
          ? { versionCodes: [String(versionCode)], status: "completed" }
          : {
              versionCodes: [String(versionCode)],
              status: "inProgress",
              userFraction: percent / 100,
            };
      await this.json("PUT", `edits/${id}/tracks/${track}`, { track, releases: [release] });
    });
  }

  halt(versionCode, fraction, track = "production") {
    return this.edit(async (id) => {
      await this.json("PUT", `edits/${id}/tracks/${track}`, {
        track,
        releases: [
          { versionCodes: [String(versionCode)], status: "halted", userFraction: fraction },
        ],
      });
    });
  }
}

// ---------------------------------------------------------------- CLI

function arg(argv, name) {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 ? argv[i + 1] : undefined;
}

function account() {
  const raw = process.env.PLAY_SERVICE_ACCOUNT_JSON;
  if (!raw)
    throw new Error(
      "PLAY_SERVICE_ACCOUNT_JSON is not set (decrypt deploy/secrets/play-publisher.enc.yaml)",
    );
  return JSON.parse(raw);
}

async function client(listing) {
  return new Play(await accessToken(account()), listing.package_name);
}

export async function main(argv) {
  const [cmd, ...rest] = argv;
  const listing = JSON.parse(read("deploy/play/listing.json"));
  const rollout = JSON.parse(read("deploy/play/rollout.json"));
  switch (cmd) {
    case "check": {
      const build = readBuild({
        gradle: read(GRADLE),
        manifest: read(MANIFEST),
        adsGradle: read(ADS_GRADLE),
        deviceSpec: read(DEVICE_SPEC),
      });
      const errors = [...checkListing(listing, build), ...checkRollout(rollout)];
      for (const e of errors) console.error(`::error::${e}`);
      return errors.length === 0 ? 0 : 1;
    }
    case "cloud-live": {
      const res = await fetch(new URL("/healthz", rest[0]));
      if (!res.ok) {
        console.error(
          `::error::${rest[0]}/healthz answered ${res.status}: the cloud release is not live, so the Hub is not uploaded`,
        );
        return 1;
      }
      return 0;
    }
    case "upload": {
      const code = await (
        await client(listing)
      ).uploadInternal(
        readFileSync(rest[0]),
        arg(rest, "notes") ?? "Arena 360 Hub",
        rollout.track_internal,
      );
      console.log(`uploaded versionCode ${code} to the ${rollout.track_internal} track`);
      return 0;
    }
    case "promote": {
      const versionCode = rest[0];
      const percent = Number(arg(rest, "percent"));
      const prelaunch = JSON.parse(readFileSync(arg(rest, "prelaunch"), "utf8"));
      const play = await client(listing);
      const live = await play.productionRelease(rollout.track_production);
      const current =
        live && live.versionCodes?.includes(String(versionCode)) && live.status === "inProgress"
          ? { percent: Math.round((live.userFraction ?? 0) * 100), since: arg(rest, "since") }
          : null;
      if (current !== null && !current.since)
        throw new Error("--since <ISO time the current stage began> is required");
      if (
        prelaunch.version_code !== undefined &&
        String(prelaunch.version_code) !== String(versionCode)
      )
        throw new Error("the pre-launch report is for another version");
      const verdict = checkPromotion({
        rollout,
        current,
        percent,
        now: new Date().toISOString(),
        prelaunch,
      });
      if (!verdict.ok) {
        console.error(`::error::${verdict.reason}`);
        return 1;
      }
      await play.setProduction(versionCode, percent, rollout.track_production);
      console.log(`production ${versionCode} is at ${percent}%`);
      return 0;
    }
    case "halt": {
      const play = await client(listing);
      const live = await play.productionRelease(rollout.track_production);
      await play.halt(rest[0], live?.userFraction ?? 0.01, rollout.track_production);
      console.log(`production ${rest[0]} halted`);
      return 0;
    }
    case "decide": {
      const metrics = {
        crash: Number(arg(rest, "crash")),
        anr: Number(arg(rest, "anr")),
        gap: Number(arg(rest, "gap")),
      };
      const d = decideHalt(metrics, rollout.halt_when);
      console.log(
        d.halt ? `HALT: ${d.reasons.join("; ")}` : "continue: every rate is within its threshold",
      );
      return d.halt ? 2 : 0;
    }
    default:
      console.error(
        "usage: play-release.mjs check | cloud-live <url> | upload <aab> | promote | halt | decide",
      );
      return 64;
  }
}

if (
  process.argv[1] &&
  realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))
) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (e) => {
      console.error(`::error::${e.message}`);
      process.exit(1);
    },
  );
}
