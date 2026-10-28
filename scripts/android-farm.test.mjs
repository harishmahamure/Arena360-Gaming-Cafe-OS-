import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  FARM,
  checkConfig,
  checkEgress,
  checkWorkflow,
  checkFrames,
  killAndRestart,
  parseDnsLog,
  parseFramestats,
  parseSyns,
  readFloor,
  runDoze,
  verifyReport,
} from "./android-farm.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const cfg = () => JSON.parse(readFileSync(join(root, FARM), "utf8"));

const op = (p95, max, count = 1000) => ({ p95_ms: p95, max_ms: max, count });
const report = (over = {}) => ({
  schema: 1,
  run: { seconds: 3600, kiosks: 20, devices: 20, rate: 5 },
  ops: { apply: op(40, 90), floor_update: op(50, 400), kiosk_update: op(55, 600) },
  lost: 0,
  duplicated: 0,
  kiosk_drops: 0,
  timer_probes: [
    { at_s: 900, due_s: 1020, late_ms: 300 },
    { at_s: 1200, due_s: 1320, late_ms: 200 },
    { at_s: 1500, due_s: 1620, late_ms: 900 },
  ],
  ...over,
});
const restart = (over = {}) => ({
  restart: {
    acknowledged: 5000,
    missing: 0,
    sessions_changed: 0,
    kiosks_resynced: 20,
    kiosks_expected: 20,
    timers_not_rebuilt: 0,
    ...over,
  },
});
const doze = { start_s: 900, end_s: 2700 };
const verify = (over = {}) =>
  verifyReport({ report: report(), restart: restart(), doze, cfg: cfg(), ...over });

test("the committed farm config says what the story says, against the real build", () => {
  assert.deepEqual(checkConfig(cfg(), readFloor()), []);
});

test("weakening the load, the Doze window or a budget fails the config check", () => {
  const weaken = (edit) => {
    const c = cfg();
    edit(c);
    return checkConfig(c, readFloor());
  };
  assert.ok(weaken((c) => (c.load.kiosks = 10)).some((e) => e.includes("load.kiosks")));
  assert.ok(weaken((c) => (c.load.intents_per_s = 1)).some((e) => e.includes("intents_per_s")));
  assert.ok(weaken((c) => (c.load.minutes = 30)).some((e) => e.includes("load.minutes")));
  assert.ok(weaken((c) => (c.doze.minutes = 10)).some((e) => e.includes("doze.minutes")));
  assert.ok(
    weaken((c) => (c.doze.start_after_min = 40)).some((e) => e.includes("before the load")),
  );
  assert.ok(weaken((c) => (c.budgets.apply.p95_ms = 250)).some((e) => e.includes("apply.p95_ms")));
  assert.ok(
    weaken((c) => (c.budgets.kiosk_update.max_ms = 5000)).some((e) => e.includes("kiosk_update")),
  );
  assert.ok(weaken((c) => (c.budgets.frame_block_ms = 500)).some((e) => e.includes("frame_block")));
  assert.ok(weaken((c) => (c.load.mix = ["start"])).some((e) => e.includes("load.mix")));
});

test("the farm must include the minSdk device with the spec's RAM and a newer one", () => {
  const dropOld = cfg();
  dropOld.devices = dropOld.devices.filter((d) => d.api_level !== 28);
  assert.ok(checkConfig(dropOld, readFloor()).some((e) => e.includes("minSdk")));
  const wrongRam = cfg();
  wrongRam.devices[0].ram_gb = 6;
  assert.ok(checkConfig(wrongRam, readFloor()).some((e) => e.includes("GB RAM")));
  const onlyOld = cfg();
  onlyOld.devices = onlyOld.devices.slice(0, 1);
  assert.ok(checkConfig(onlyOld, readFloor()).some((e) => e.includes("newer Android")));
});

test("a run inside every budget passes", () => {
  assert.deepEqual(verify(), []);
});

test("each budget, the lost and duplicated counts and the kiosk drops fail the release", () => {
  const apply = verify({ report: report({ ops: { ...report().ops, apply: op(130, 200) } }) });
  assert.ok(apply.some((e) => e.includes("apply p95 130")));
  const floor = verify({
    report: report({ ops: { ...report().ops, floor_update: op(50, 2600) } }),
  });
  assert.ok(floor.some((e) => e.includes("floor_update slowest")));
  assert.ok(verify({ report: report({ lost: 2 }) }).some((e) => e.includes("never reached")));
  assert.ok(verify({ report: report({ duplicated: 1 }) }).some((e) => e.includes("twice")));
  assert.ok(verify({ report: report({ kiosk_drops: 3 }) }).some((e) => e.includes("lost their")));
});

test("a short run, a slow load or fewer kiosks do not count as the proof", () => {
  const e = verify({ report: report({ run: { seconds: 60, kiosks: 5, rate: 1 } }) });
  assert.ok(e.some((x) => x.includes("5 kiosks")));
  assert.ok(e.some((x) => x.includes("1/s")));
  assert.ok(e.some((x) => x.includes("60 s")));
});

test("a missing operation is an error, and a later story's operation is checked without code", () => {
  assert.ok(
    verify({ report: report({ ops: {} }) }).some((e) => e.includes("no samples for apply")),
  );
  const c = cfg();
  c.budgets.daily_rollup = { p95_ms: 5000 };
  assert.ok(verify({ cfg: c }).some((e) => e.includes("no samples for daily_rollup")));
  const r = report();
  r.ops.daily_rollup = op(6200, 6500);
  assert.ok(verify({ cfg: c, report: r }).some((e) => e.includes("daily_rollup p95 6200")));
  r.ops.daily_rollup = op(4000, 4500);
  assert.deepEqual(verify({ cfg: c, report: r }), []);
});

test("time-ups must fire on time, and enough of them must fall inside the Doze window", () => {
  const late = report();
  late.timer_probes[1].late_ms = 9000;
  assert.ok(verify({ report: late }).some((e) => e.includes("9000 ms late")));
  const never = report();
  never.timer_probes[0].late_ms = null;
  assert.ok(verify({ report: never }).some((e) => e.includes("never fired")));
  const outside = report();
  outside.timer_probes = outside.timer_probes.map((p) => ({ ...p, due_s: 100 }));
  assert.ok(verify({ report: outside }).some((e) => e.includes("inside the Doze window")));
});

test("a probe still running at the end is left to the post-kill check", () => {
  const r = report();
  r.timer_probes.push({ at_s: 3590, due_s: 3710, late_ms: null });
  assert.deepEqual(verify({ report: r }), []);
});

test("the post-kill phase fails on a missing change, a different session set, a kiosk or a timer", () => {
  assert.ok(verify({ restart: restart({ missing: 3 }) }).some((e) => e.includes("not there")));
  assert.ok(
    verify({ restart: restart({ sessions_changed: 1 }) }).some((e) => e.includes("differ")),
  );
  assert.ok(
    verify({ restart: restart({ kiosks_resynced: 17 }) }).some((e) => e.includes("17 of 20")),
  );
  assert.ok(
    verify({ restart: restart({ timers_not_rebuilt: 1 }) }).some((e) => e.includes("rebuilt")),
  );
});

const FRAMES = (rows) =>
  [
    "Applications Graphics Acceleration Info:",
    "---PROFILEDATA---",
    "Flags,FrameTimelineVsyncId,IntendedVsync,Vsync,InputEventId,HandleInputStart,AnimationStart,PerformTraversalsStart,DrawStart,FrameDeadline,FrameInterval,SyncQueued,SyncStart,IssueDrawCommandsStart,SwapBuffers,FrameCompleted,DequeueBufferDuration,QueueBufferDuration,GpuCompleted,SwapBuffersCompleted",
    ...rows.map(
      ([flags, begin, end]) =>
        `${flags},1,${begin},${begin},0,0,0,0,0,0,0,0,0,0,0,${end},0,0,0,${end}`,
    ),
    "---PROFILEDATA---",
  ].join("\n");

test("frame durations are FrameCompleted minus IntendedVsync, skipping flagged frames", () => {
  const text = FRAMES([
    [0, 1_000_000_000, 1_016_000_000],
    [0, 2_000_000_000, 2_150_000_000],
    [1, 3_000_000_000, 4_000_000_000],
  ]);
  assert.deepEqual(parseFramestats(text), [16, 150]);
});

test("a frame over 100 ms fails and no frames at all fails", () => {
  assert.deepEqual(checkFrames([16, 90, 100], 100), []);
  assert.ok(checkFrames([16, 150, 220], 100)[0].includes("2 of 3 frames"));
  assert.ok(checkFrames([], 100)[0].includes("not measured"));
});

const DNS = [
  "Oct 28 10:00:01 dnsmasq[1]: reply googleads.g.doubleclick.net is 142.250.1.1",
  "Oct 28 10:00:02 dnsmasq[1]: reply pagead2.googlesyndication.com is 142.250.1.2",
  "Oct 28 10:00:03 dnsmasq[1]: reply tracker.example.com is 203.0.113.9",
  "Oct 28 10:00:03 dnsmasq[1]: reply shared.example.com is 142.250.1.3",
  "Oct 28 10:00:03 dnsmasq[1]: reply googleads.g.doubleclick.net is 142.250.1.3",
].join("\n");
const SYN = (dst) => `10:00:00.1 IP 192.168.8.21.51234 > ${dst}: Flags [S], seq 1, win 65535`;

test("syns and dns names are parsed", () => {
  assert.deepEqual(parseSyns([SYN("1.1.1.1.443"), "10:00 IP a.1 > b.2: Flags [.]"].join("\n")), [
    "1.1.1.1:443",
  ]);
  assert.deepEqual([...parseDnsLog(DNS).get("142.250.1.3")].sort(), [
    "googleads.g.doubleclick.net",
    "shared.example.com",
  ]);
});

test("only the probe, the LAN and ad domains may be dialled", () => {
  const c = cfg();
  const run = (...dsts) =>
    checkEgress({
      syns: parseSyns(dsts.map(SYN).join("\n")),
      dns: parseDnsLog(DNS),
      probe: c.egress.probe,
      adDomains: c.egress.ad_domains,
    });
  const ok = run("1.1.1.1.443", "8.8.8.8.443", "142.250.1.1.443", "192.168.8.30.8443");
  assert.deepEqual(ok.errors, []);
  assert.equal(ok.ads, 1);
  const bad = run("203.0.113.9.443", "198.51.100.7.443", "142.250.1.3.443", "1.1.1.1.80");
  assert.equal(bad.errors.length, 4);
  assert.ok(bad.errors[0].includes("tracker.example.com"));
  assert.ok(bad.errors[2].includes("shared.example.com"));
});

const fakeAdb = ({ idle = () => "IDLE", services = "isForeground=true" } = {}) => {
  const calls = [];
  const adb = (...args) => {
    calls.push(args.join(" "));
    if (args.includes("get")) return `${idle(calls.length)}\n`;
    if (args.includes("services")) return services;
    return "";
  };
  return { adb, calls };
};
const clock = () => {
  let t = 1_000_000;
  return { now: () => t, sleep: async (ms) => void (t += ms) };
};

test("Doze is forced with the screen off, watched every minute and released after", async () => {
  const { adb, calls } = fakeAdb();
  const { now, sleep } = clock();
  let health = 0;
  const out = await runDoze({
    adb,
    package: "in.arena360.hub",
    minutes: 30,
    healthy: async () => (health += 1) > 0,
    now,
    sleep,
  });
  assert.deepEqual(out.problems, []);
  assert.equal(health, 30);
  assert.equal(out.end_ms - out.start_ms, 30 * 60_000);
  assert.ok(
    calls.indexOf("shell input keyevent KEYCODE_SLEEP") <
      calls.indexOf("shell dumpsys deviceidle force-idle"),
  );
  assert.ok(calls.at(-1).includes("KEYCODE_WAKEUP"));
  assert.ok(calls.includes("shell dumpsys deviceidle unforce"));
});

test("leaving Doze, a service that is not foreground or a Hub that stops answering are problems", async () => {
  const { adb } = fakeAdb({
    idle: (n) => (n === 8 ? "ACTIVE" : "IDLE"),
    services: "ServiceRecord",
  });
  const { now, sleep } = clock();
  let n = 0;
  const out = await runDoze({
    adb,
    package: "in.arena360.hub",
    minutes: 3,
    healthy: async () => (n += 1) !== 2,
    now,
    sleep,
  });
  assert.ok(out.problems.some((p) => p.includes("not a foreground service")));
  assert.ok(out.problems.some((p) => p.includes("stopped answering")));
});

test("the kill force-stops the app and launches it again", () => {
  const { adb, calls } = fakeAdb();
  killAndRestart({ adb, package: "in.arena360.hub" });
  assert.deepEqual(calls, [
    "shell am force-stop in.arena360.hub",
    "shell monkey -p in.arena360.hub -c android.intent.category.LAUNCHER 1",
  ]);
});

test("the workflow matrix runs exactly the farm devices", () => {
  const text = readFileSync(join(root, ".github/workflows/android-farm.yml"), "utf8");
  assert.deepEqual(checkWorkflow(cfg(), text), []);
  const c = cfg();
  c.devices.push({ ...c.devices[0], id: "android-extra" });
  assert.ok(checkWorkflow(c, text).some((e) => e.includes("android-extra")));
  assert.ok(checkWorkflow(cfg(), text.replace("android-latest]", "android-old]")).length === 2);
});
