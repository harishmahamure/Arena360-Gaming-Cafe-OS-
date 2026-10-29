import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { credentialTargets, drive, main, scan } from "./launcher-residue.mjs";

/** A player profile and a Steam folder with the given files (path relative to the root). */
function rig(files) {
  const root = mkdtempSync(join(tmpdir(), "residue-"));
  const profile = join(root, "player");
  const steam = join(root, "steam");
  mkdirSync(join(steam, "config"), { recursive: true });
  mkdirSync(join(profile, "AppData", "Local"), { recursive: true });
  mkdirSync(join(profile, "AppData", "Roaming"), { recursive: true });
  for (const [rel, content] of Object.entries(files)) {
    const file = join(root, rel);
    mkdirSync(join(file, ".."), { recursive: true });
    writeFileSync(file, content);
  }
  return { profile, steam };
}

test("a clean PC has no residue", () => {
  assert.deepEqual(scan(rig({})), []);
});

test("every launcher's remembered login is found and named, never its contents", () => {
  const where = rig({
    "steam/config/loginusers.vdf": "secret",
    "steam/ssfn123456": "x",
    "player/AppData/Local/EpicGamesLauncher/Saved/Config/Windows/GameUserSettings.ini": "token",
    "player/AppData/Local/EpicGamesLauncher/Saved/webcache_4147/Cookies": "x",
    "player/AppData/Local/Riot Games/Riot Client/Data/RiotGamesPrivateSettings.yaml": "x",
    "player/AppData/Roaming/discord/Local Storage/leveldb/000003.log": "x",
  });
  assert.deepEqual(scan(where).sort(), [
    "Discord: discord",
    "Epic: GameUserSettings.ini",
    "Epic: webcache_4147",
    "Riot: Riot Client/Data",
    "Steam: config/loginusers.vdf",
    "Steam: ssfn123456",
  ]);
});

test("a browser's reset profile is clean but its cookies, saved logins and history are not", () => {
  const clean = rig({
    "player/AppData/Local/Google/Chrome/User Data/Default/Preferences": "{}",
    "player/AppData/Local/Google/Chrome/User Data/Default/Cookies": "",
  });
  assert.deepEqual(scan(clean), [], "an empty Cookies file holds no session");
  const dirty = rig({
    "player/AppData/Local/Google/Chrome/User Data/Default/Network/Cookies": "session",
    "player/AppData/Local/Microsoft/Edge/User Data/Profile 1/Login Data": "pw",
    "player/AppData/Roaming/Mozilla/Firefox/Profiles/x.default/logins.json": "pw",
  });
  assert.deepEqual(scan(dirty).sort(), [
    "Chrome: Default/Network/Cookies",
    "Edge: Profile 1/Login Data",
    "Firefox: Profiles/x.default/logins.json",
  ]);
});

test("saved Windows credentials of a launcher or browser are residue and others are not", () => {
  const listing = [
    "Currently stored credentials:",
    "",
    "    Target: LegacyGeneric:target=SteamUser",
    "    Type: Generic",
    "    Target: WindowsLive:target=virtualapp/didlogical",
    "    Target: LegacyGeneric:target=Discord/Token",
    "    Target: LegacyGeneric:target=Google Chrome Safe Storage",
  ].join("\r\n");
  assert.deepEqual(credentialTargets(listing), [
    "Credential Manager: LegacyGeneric:target=SteamUser",
    "Credential Manager: LegacyGeneric:target=Discord/Token",
    "Credential Manager: LegacyGeneric:target=Google Chrome Safe Storage",
  ]);
  assert.deepEqual(credentialTargets(listing, [/Safe Storage/i]).length, 2);
});

test("the driver signs in, ends by each path in turn and scans after every session", () => {
  const ran = [];
  const result = drive({
    sessions: 4,
    loginCmd: "login {n}",
    endCmds: ["staff {n}", "timeup {n}", "done {n}"],
    scanNow: () => [],
    runCmd: (cmd) => (ran.push(cmd), 0),
  });
  assert.deepEqual(ran, [
    "login {n}",
    "staff {n}",
    "login {n}",
    "timeup {n}",
    "login {n}",
    "done {n}",
    "login {n}",
    "staff {n}",
  ]);
  assert.deepEqual(result, { sessions: 4, residue: [] });
});

test("a residual is recorded against its session", () => {
  let n = 0;
  const result = drive({
    sessions: 3,
    loginCmd: "l",
    endCmds: ["e"],
    scanNow: () => (++n === 2 ? ["Steam: config/loginusers.vdf"] : []),
    runCmd: () => 0,
  });
  assert.deepEqual(result.residue, ["session 2: Steam: config/loginusers.vdf"]);
});

test("a session whose login or end command failed does not count and is reported", () => {
  const result = drive({
    sessions: 3,
    loginCmd: "login",
    endCmds: ["end"],
    scanNow: () => [],
    runCmd: (cmd, n) => (cmd === "login" && n === 2 ? 1 : 0),
  });
  assert.equal(result.sessions, 2);
  assert.deepEqual(result.residue, ["session 2: the login command failed"]);
});

test("main refuses a run that signs nothing in", () => {
  const { profile } = rig({});
  assert.equal(main(["--profile", profile]), 2);
  assert.equal(main([]), 2);
});

test("main --once exits 1 on residue", () => {
  const dirty = rig({ "steam/config/loginusers.vdf": "x" });
  assert.equal(main(["--once", "--profile", dirty.profile, "--steam", dirty.steam]), 1);
});
