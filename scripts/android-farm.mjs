#!/usr/bin/env node
// Story 4.17: the device-farm job for the Android Hub. The load itself is the `android_fleet`
// test (crates/arena-hub/tests/android_fleet.rs); this script holds everything around it that is
// plain logic so it can be tested without a device:
//
//   check                         the farm config still says what the story says
//   verify <report> [<restart>] [<doze>]
//                                 the load report against the budgets; exit 1 fails the release
//   frames <gfxinfo.txt>          no UI frame blocked longer than the limit (ads loading)
//   egress <syn.log> <dns.log>    no connection other than the probe and ad requests (WAN down)
//   doze <serial> [--minutes n] [--out doze.json]
//                                 force Doze with the screen off and watch the service serve
//   kill <serial>                 force-stop the app and start it again (the restart check)
//
// The budgets live in deploy/android-farm/farm.json; a later story adds one entry there.

import { execFileSync } from "node:child_process";
import { readFileSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DEVICE_SPEC, GRADLE, MANIFEST, ADS_GRADLE, readBuild } from "./play-release.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");
export const FARM = "deploy/android-farm/farm.json";

/** The story's floors: a config that weakens any of them fails `check`. */
export const FLOORS = {
  kiosks: 20,
  intents_per_s: 5,
  minutes: 60,
  dozeMinutes: 30,
  applyP95Ms: 100,
  updateMs: 2000,
  frameBlockMs: 100,
};

const isObj = (v) => v && typeof v === "object" && !Array.isArray(v);

/** Problems with the farm config, as a list of sentences (empty when it is fine). */
export function checkConfig(cfg, build) {
  const errors = [];
  const load = cfg.load ?? {};
  if (!(load.kiosks >= FLOORS.kiosks)) errors.push(`load.kiosks must be at least ${FLOORS.kiosks}`);
  if (!(load.devices >= FLOORS.kiosks))
    errors.push(`load.devices must be at least ${FLOORS.kiosks} (one per kiosk)`);
  if (!(load.intents_per_s >= FLOORS.intents_per_s))
    errors.push(`load.intents_per_s must be at least ${FLOORS.intents_per_s}`);
  if (!(load.minutes >= FLOORS.minutes))
    errors.push(`load.minutes must be at least ${FLOORS.minutes}`);
  for (const kind of ["start", "pause", "resume", "extend", "transfer", "end", "settle"])
    if (!load.mix?.includes(kind)) errors.push(`load.mix must include ${kind}`);
  const doze = cfg.doze ?? {};
  if (!(doze.minutes >= FLOORS.dozeMinutes))
    errors.push(`doze.minutes must be at least ${FLOORS.dozeMinutes}`);
  if (!(doze.start_after_min >= 1)) errors.push("doze.start_after_min must be at least 1");
  if (!(doze.start_after_min + doze.minutes < load.minutes))
    errors.push("the Doze window must end before the load does");
  if (!(load.timer_probe_every_s > 0 && load.timer_probe_minutes >= 1))
    errors.push("timer probes must be on (timer_probe_every_s, timer_probe_minutes)");
  if (!(doze.min_probes_inside >= 1)) errors.push("doze.min_probes_inside must be at least 1");

  const b = cfg.budgets ?? {};
  if (!(b.apply?.p95_ms <= FLOORS.applyP95Ms))
    errors.push(`budgets.apply.p95_ms must be at most ${FLOORS.applyP95Ms}`);
  for (const op of ["floor_update", "kiosk_update"])
    if (!(b[op]?.max_ms <= FLOORS.updateMs))
      errors.push(`budgets.${op}.max_ms must be at most ${FLOORS.updateMs}`);
  if (!(b.frame_block_ms <= FLOORS.frameBlockMs))
    errors.push(`budgets.frame_block_ms must be at most ${FLOORS.frameBlockMs}`);
  if (!(b.timer_late_ms > 0)) errors.push("budgets.timer_late_ms must be set");
  for (const [name, v] of Object.entries(b))
    if (isObj(v) && !("p95_ms" in v || "max_ms" in v))
      errors.push(`budgets.${name} needs p95_ms or max_ms`);

  const devices = cfg.devices ?? [];
  const oldest = devices.find((d) => d.api_level === build.minSdk);
  if (!oldest) errors.push(`no farm device runs the minSdk (API ${build.minSdk})`);
  else if (oldest.ram_gb !== build.minRamGb)
    errors.push(`the minSdk device must have ${build.minRamGb} GB RAM, the spec floor`);
  if (!devices.some((d) => d.api_level > build.minSdk))
    errors.push("no farm device runs a newer Android than the minSdk");
  for (const d of devices)
    if (!d.runner_labels?.includes(d.id))
      errors.push(`device ${d.id} needs its id as a runner label`);

  if (!(cfg.egress?.probe?.length > 0)) errors.push("egress.probe must list the probe endpoints");
  return errors;
}

/** The workflow must run every farm device and nothing the config does not list. */
export function checkWorkflow(cfg, text) {
  const errors = [];
  const m = /matrix:\s*\n\s*device:\s*\[([^\]]*)\]/.exec(text);
  const ids = m ? m[1].split(",").map((s) => s.trim()) : [];
  if (!m) errors.push("the workflow has no device matrix");
  for (const d of cfg.devices)
    if (!ids.includes(d.id)) errors.push(`the workflow does not run ${d.id}`);
  for (const id of ids)
    if (!cfg.devices.some((d) => d.id === id))
      errors.push(`the workflow runs ${id}, not in the config`);
  return errors;
}

/** The build facts the floor is checked against, read from the sources. */
export function readFloor() {
  return readBuild({
    gradle: read(GRADLE),
    manifest: read(MANIFEST),
    adsGradle: read(ADS_GRADLE),
    deviceSpec: read(DEVICE_SPEC),
  });
}

/**
 * The load report (written by `android_fleet`) against the budgets. `doze` is
 * `{ start_s, end_s }` of the Doze window in seconds from the start of the load, when the
 * Doze scenario ran. `restart` is the report of the post-kill phase.
 */
export function verifyReport({ report, restart, doze, cfg }) {
  const errors = [];
  const b = cfg.budgets;
  if (report?.schema !== 1) return ["the load report is missing or not schema 1"];
  const run = report.run ?? {};
  if (run.kiosks < cfg.load.kiosks) errors.push(`only ${run.kiosks} kiosks ran`);
  if (run.rate < cfg.load.intents_per_s) errors.push(`the load ran at ${run.rate}/s`);
  if (run.seconds < cfg.load.minutes * 60)
    errors.push(`the load ran ${run.seconds} s, the story asks for ${cfg.load.minutes * 60} s`);

  for (const [name, limit] of Object.entries(b)) {
    if (!isObj(limit)) continue;
    const op = report.ops?.[name];
    if (!op || !(op.count > 0)) {
      errors.push(`no samples for ${name}`);
      continue;
    }
    if ("p95_ms" in limit && !(op.p95_ms <= limit.p95_ms))
      errors.push(`${name} p95 ${op.p95_ms} ms is over ${limit.p95_ms} ms`);
    if ("max_ms" in limit && !(op.max_ms <= limit.max_ms))
      errors.push(`${name} slowest ${op.max_ms} ms is over ${limit.max_ms} ms`);
  }
  if (report.lost !== 0) errors.push(`${report.lost} committed intents never reached a screen`);
  if (report.duplicated !== 0) errors.push(`${report.duplicated} intents were delivered twice`);
  if (report.kiosk_drops !== 0) errors.push(`${report.kiosk_drops} kiosks lost their connection`);

  const probes = report.timer_probes ?? [];
  for (const p of probes) {
    if (p.late_ms !== null && p.late_ms > b.timer_late_ms)
      errors.push(`a time-up fired ${p.late_ms} ms late (limit ${b.timer_late_ms} ms)`);
    // One due well before the load ended must have fired; later ones are checked after the kill.
    if (p.late_ms === null && p.due_s < run.seconds - 30)
      errors.push(`the time-up due at ${Math.round(p.due_s)} s never fired`);
  }
  if (doze) {
    const inside = probes.filter(
      (p) => p.late_ms !== null && p.due_s >= doze.start_s && p.due_s <= doze.end_s,
    );
    if (inside.length < cfg.doze.min_probes_inside)
      errors.push(
        `only ${inside.length} time-ups fired inside the Doze window, need ${cfg.doze.min_probes_inside}`,
      );
  }

  if (restart) {
    const r = restart.restart ?? {};
    if (r.missing !== 0)
      errors.push(`${r.missing} acknowledged intents were not there after the kill`);
    if (r.sessions_changed !== 0)
      errors.push(`${r.sessions_changed} sessions differ after the kill`);
    if (r.kiosks_resynced !== r.kiosks_expected)
      errors.push(`${r.kiosks_resynced} of ${r.kiosks_expected} kiosks synced after the kill`);
    if (r.timers_not_rebuilt !== 0)
      errors.push(`${r.timers_not_rebuilt} timers were not rebuilt after the kill`);
  }
  return errors;
}

/**
 * The per-frame durations in ms of `dumpsys gfxinfo <pkg> framestats`: `FrameCompleted` minus
 * `IntendedVsync` of every frame that was not flagged as skipped.
 */
export function parseFramestats(text) {
  const out = [];
  let cols = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line === "---PROFILEDATA---") {
      cols = null;
      continue;
    }
    if (line.startsWith("Flags,")) {
      cols = line.split(",");
      continue;
    }
    if (!cols || !/^\d/.test(line)) continue;
    const f = line.split(",");
    const flags = Number(f[cols.indexOf("Flags")]);
    const begin = Number(f[cols.indexOf("IntendedVsync")]);
    const end = Number(f[cols.indexOf("FrameCompleted")]);
    if (flags !== 0 || !Number.isFinite(begin) || !Number.isFinite(end) || end <= begin) continue;
    out.push((end - begin) / 1e6);
  }
  return out;
}

export function checkFrames(durations, limitMs) {
  if (durations.length === 0) return ["gfxinfo had no frames: the UI was not measured"];
  const worst = Math.max(...durations);
  const blocked = durations.filter((d) => d > limitMs).length;
  return blocked === 0
    ? []
    : [
        `${blocked} of ${durations.length} frames took over ${limitMs} ms (worst ${worst.toFixed(1)} ms)`,
      ];
}

/** `dnsmasq` log lines `reply <name> is <ip>` as a map of ip to the names it answered for. */
export function parseDnsLog(text) {
  const names = new Map();
  for (const line of text.split(/\r?\n/)) {
    const m = /\b(?:reply|cached)\s+(\S+)\s+is\s+([0-9a-fA-F.:]+)\s*$/.exec(line);
    if (!m || m[2] === "<CNAME>") continue;
    const set = names.get(m[2].toLowerCase()) ?? new Set();
    set.add(m[1].toLowerCase());
    names.set(m[2].toLowerCase(), set);
  }
  return names;
}

/** `tcpdump -n` SYN lines (`IP a.b.c.d.p > w.x.y.z.q: Flags [S]`) as destination `ip:port`. */
export function parseSyns(text) {
  const out = [];
  for (const line of text.split(/\r?\n/)) {
    if (!/Flags \[S\]/.test(line)) continue;
    const m = /\bIP6? (\S+) > (\S+): /.exec(line);
    if (!m) continue;
    const dst = m[2];
    const i = dst.lastIndexOf(".");
    out.push(`${dst.slice(0, i)}:${dst.slice(i + 1)}`);
  }
  return out;
}

const isPrivate = (ip) =>
  /^(10\.|127\.|192\.168\.|169\.254\.)/.test(ip) || /^172\.(1[6-9]|2\d|3[01])\./.test(ip);

/**
 * Every outbound connection of the device must be to the connectivity probe's endpoints or to
 * an address whose DNS names are all ad domains; LAN addresses are not outbound.
 */
export function checkEgress({ syns, dns, probe, adDomains }) {
  const errors = [];
  const isAd = (name) => adDomains.some((d) => name === d || name.endsWith(`.${d}`));
  let ads = 0;
  for (const dst of new Set(syns)) {
    const ip = dst.slice(0, dst.lastIndexOf(":"));
    if (isPrivate(ip) || probe.includes(dst)) continue;
    const names = dns.get(ip.toLowerCase());
    if (names && names.size > 0 && [...names].every(isAd)) {
      ads += 1;
      continue;
    }
    errors.push(
      `unexpected outbound connection to ${dst}${names ? ` (${[...names].join(", ")})` : ""}`,
    );
  }
  return { errors, ads };
}

// ------------------------------------------------------------------ device control

/** The default `adb` runner: returns stdout. */
export function realAdb(serial) {
  return (...args) =>
    execFileSync("adb", ["-s", serial, ...args], { encoding: "utf8", timeout: 60_000 });
}

/**
 * Forces Doze with the screen off and watches for `minutes`: every `everyS` seconds the device
 * must still be in deep idle (re-forced when the platform leaves it), the app's service must be
 * a foreground service and `healthy()` (the Hub's /healthz over the forwarded port) must pass.
 * Returns the problems and the window in epoch ms.
 */
export async function runDoze({ adb, package: pkg, minutes, everyS = 60, healthy, now, sleep }) {
  const problems = [];
  const start = now();
  adb("shell", "dumpsys", "battery", "unplug");
  adb("shell", "dumpsys", "deviceidle", "enable", "deep");
  adb("shell", "input", "keyevent", "KEYCODE_SLEEP");
  adb("shell", "dumpsys", "deviceidle", "force-idle");
  const end = start + minutes * 60_000;
  let ticks = 0;
  while (now() < end) {
    await sleep(everyS * 1000);
    ticks += 1;
    const state = adb("shell", "dumpsys", "deviceidle", "get", "deep").trim();
    if (state !== "IDLE") {
      problems.push(`left deep Doze (${state}) after ${ticks} checks; forced back`);
      adb("shell", "dumpsys", "deviceidle", "force-idle");
    }
    const services = adb("shell", "dumpsys", "activity", "services", pkg);
    if (!/isForeground=true/.test(services))
      problems.push(`the Hub service was not a foreground service at check ${ticks}`);
    if (!(await healthy()))
      problems.push(`the Hub stopped answering while in Doze at check ${ticks}`);
  }
  adb("shell", "dumpsys", "deviceidle", "unforce");
  adb("shell", "dumpsys", "battery", "reset");
  adb("shell", "input", "keyevent", "KEYCODE_WAKEUP");
  return { problems, start_ms: start, end_ms: now() };
}

/** Kills the app the way Android does under memory pressure, then launches it again. */
export function killAndRestart({ adb, package: pkg }) {
  adb("shell", "am", "force-stop", pkg);
  adb("shell", "monkey", "-p", pkg, "-c", "android.intent.category.LAUNCHER", "1");
}

// ------------------------------------------------------------------ CLI

const json = (p) => JSON.parse(readFileSync(p, "utf8"));
const flag = (args, name) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : undefined;
};

export async function main(argv) {
  const [cmd, ...rest] = argv;
  const cfg = json(join(root, FARM));
  switch (cmd) {
    case "check": {
      const errors = [
        ...checkConfig(cfg, readFloor()),
        ...checkWorkflow(cfg, read(".github/workflows/android-farm.yml")),
      ];
      for (const e of errors) console.error(`::error::${e}`);
      if (errors.length === 0) console.log("android farm config ok");
      return errors.length === 0 ? 0 : 1;
    }
    case "verify": {
      const [reportPath, restartPath, dozePath] = rest;
      const doze = dozePath ? json(dozePath) : undefined;
      const errors = verifyReport({
        report: json(reportPath),
        restart: restartPath ? json(restartPath) : undefined,
        doze: doze && { start_s: doze.start_s, end_s: doze.end_s },
        cfg,
      });
      if (doze?.problems?.length) errors.push(...doze.problems);
      for (const e of errors) console.error(`::error::${e}`);
      if (errors.length === 0) console.log("android farm run within every budget");
      return errors.length === 0 ? 0 : 1;
    }
    case "frames": {
      const errors = checkFrames(
        parseFramestats(readFileSync(rest[0], "utf8")),
        cfg.budgets.frame_block_ms,
      );
      for (const e of errors) console.error(`::error::${e}`);
      return errors.length === 0 ? 0 : 1;
    }
    case "egress": {
      const { errors, ads } = checkEgress({
        syns: parseSyns(readFileSync(rest[0], "utf8")),
        dns: parseDnsLog(readFileSync(rest[1], "utf8")),
        probe: cfg.egress.probe,
        adDomains: cfg.egress.ad_domains,
      });
      for (const e of errors) console.error(`::error::${e}`);
      console.log(`${ads} ad destinations seen`);
      return errors.length === 0 ? 0 : 1;
    }
    case "doze": {
      const serial = rest[0];
      const minutes = Number(flag(rest, "minutes") ?? cfg.doze.minutes);
      const port = flag(rest, "health-port");
      const healthy = async () => {
        try {
          const r = await fetch(`http://127.0.0.1:${port}/healthz`, {
            signal: AbortSignal.timeout(5000),
          });
          return r.ok && (await r.json()).role === "primary";
        } catch {
          return false;
        }
      };
      const started = Date.now();
      const out = await runDoze({
        adb: realAdb(serial),
        package: cfg.package,
        minutes,
        healthy,
        now: Date.now,
        sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
      });
      const loadStart = Number(flag(rest, "load-start-ms") ?? started);
      const result = {
        start_s: (out.start_ms - loadStart) / 1000,
        end_s: (out.end_ms - loadStart) / 1000,
        problems: out.problems,
      };
      writeFileSync(flag(rest, "out") ?? "doze.json", JSON.stringify(result, null, 2));
      for (const p of out.problems) console.error(`::error::${p}`);
      return out.problems.length === 0 ? 0 : 1;
    }
    case "kill": {
      killAndRestart({ adb: realAdb(rest[0]), package: cfg.package });
      return 0;
    }
    default:
      console.error(
        "usage: android-farm.mjs check | verify <report> [<restart> [<doze>]] | frames <gfxinfo> | egress <syn.log> <dns.log> | doze <serial> | kill <serial>",
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
