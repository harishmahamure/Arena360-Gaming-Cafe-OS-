#!/usr/bin/env node
// Story 6.38: the launcher-logout proof. After a session ends, nothing of the player's Steam,
// Epic, Riot, Discord, Chrome or Edge login may be left on the PC, and no saved Windows
// credential. The paths are the ones the kiosk service's sign-out removes (Story 6.12,
// `cleanup/signout.rs`), read from outside it, so a change there is caught here.
//
//   node scripts/launcher-residue.mjs --os "Windows 11" --profile C:\Users\player --steam "C:\Program Files (x86)\Steam" \
//        --sessions 20 --login-cmd "<signs the real accounts in>" --end-cmd "<ends by staff>" --end-cmd "<ends at time-up>" ... --out target/journey/residue-win11.json
//   node scripts/launcher-residue.mjs --once --profile ... --steam ...      (scan once and print)
//
// The driver runs, for each of `--sessions` sessions, `--login-cmd` (the rig's script that signs
// every launcher and browser in with its real account), then one `--end-cmd` (the end paths are
// used in turn: staff end, time-up, I'm done, forced end, ...), then scans. `{n}` in a command is
// the session number. A residual anywhere is recorded against its session and the exit code is
// 1; scripts/journey-report.mjs fails the release on it. A rig that has not signed anything in
// proves nothing, so a run with no `--login-cmd` is refused unless `--once`.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, realpathSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/** A browser file that holds a login or a session: any non-empty one is a residual. */
const BROWSER_FILES = ["Cookies", "Login Data", "Login Data For Account", "Web Data", "History"];
const FIREFOX_FILES = ["cookies.sqlite", "logins.json", "key4.db", "places.sqlite"];

const present = (p) => {
  try {
    return existsSync(p);
  } catch {
    return false;
  }
};

const size = (p) => {
  try {
    return statSync(p).size;
  } catch {
    return 0;
  }
};

/** `dir` entries whose name starts with `prefix`. */
const startingWith = (dir, prefix) => {
  try {
    return readdirSync(dir).filter((n) => n.startsWith(prefix));
  } catch {
    return [];
  }
};

/** Every file below `dir` named `name` with content (two levels of profile folders are enough). */
function loginFiles(dir, names, depth = 3) {
  const found = [];
  let entries = [];
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return found;
  }
  for (const e of entries) {
    const p = join(dir, e.name);
    if (e.isDirectory()) {
      if (depth > 0) found.push(...loginFiles(p, names, depth - 1));
    } else if (names.includes(e.name) && size(p) > 0) {
      found.push(p);
    }
  }
  return found;
}

/** The residuals of one scan: strings that name the launcher and the kind of file, never contents. */
export function scan({ profile, steam }) {
  const local = join(profile, "AppData", "Local");
  const roaming = join(profile, "AppData", "Roaming");
  const out = [];
  const flag = (who, what, p) => {
    if (present(p)) out.push(`${who}: ${what}`);
  };

  if (steam) {
    flag("Steam", "config/loginusers.vdf", join(steam, "config", "loginusers.vdf"));
    flag("Steam", "config/config.vdf", join(steam, "config", "config.vdf"));
    for (const n of startingWith(steam, "ssfn")) out.push(`Steam: ${n}`);
  }
  flag("Steam", "htmlcache", join(local, "Steam", "htmlcache"));

  flag(
    "Epic",
    "GameUserSettings.ini",
    join(local, "EpicGamesLauncher", "Saved", "Config", "Windows", "GameUserSettings.ini"),
  );
  for (const n of startingWith(join(local, "EpicGamesLauncher", "Saved"), "webcache")) {
    out.push(`Epic: ${n}`);
  }
  flag("Riot", "Riot Client/Data", join(local, "Riot Games", "Riot Client", "Data"));
  for (const d of ["discord", "discordptb", "discordcanary"]) flag("Discord", d, join(roaming, d));

  for (const [who, dir] of [
    ["Chrome", join(local, "Google", "Chrome", "User Data")],
    ["Edge", join(local, "Microsoft", "Edge", "User Data")],
  ]) {
    for (const f of loginFiles(dir, BROWSER_FILES))
      out.push(`${who}: ${f.slice(dir.length + 1).replaceAll("\\", "/")}`);
  }
  for (const dir of [join(roaming, "Mozilla", "Firefox"), join(local, "Mozilla", "Firefox")]) {
    for (const f of loginFiles(dir, FIREFOX_FILES))
      out.push(`Firefox: ${f.slice(dir.length + 1).replaceAll("\\", "/")}`);
  }
  return out;
}

const CREDENTIAL = /steam|epic|riot|discord|google|chrome|msedge|microsoftedge/i;

/** The saved Windows credentials of a launcher or browser in `cmdkey /list` output. */
export function credentialTargets(listing, ignore = []) {
  return listing
    .split(/\r?\n/)
    .map((l) => l.match(/^\s*Target:\s*(.+?)\s*$/)?.[1])
    .filter((t) => t && CREDENTIAL.test(t) && !ignore.some((re) => re.test(t)))
    .map((t) => `Credential Manager: ${t}`);
}

function savedCredentials(ignore) {
  const r = spawnSync("cmdkey", ["/list"], { encoding: "utf8", shell: false });
  return r.status === 0 ? credentialTargets(r.stdout ?? "", ignore) : [];
}

/** Runs one rig command; `{n}` is the session number. Returns its exit code. */
function run(cmd, n) {
  return spawnSync(cmd.replaceAll("{n}", String(n)), { shell: true, stdio: "inherit" }).status ?? 1;
}

/** The driver: `sessions` sessions, each logged in, ended by the next end path, then scanned. */
export function drive({ sessions, loginCmd, endCmds, scanNow, runCmd = run }) {
  const residue = [];
  const failed = [];
  for (let n = 1; n <= sessions; n++) {
    if (runCmd(loginCmd, n) !== 0) {
      failed.push(`session ${n}: the login command failed`);
      continue;
    }
    const end = endCmds[(n - 1) % endCmds.length];
    if (runCmd(end, n) !== 0) {
      failed.push(`session ${n}: the end command failed`);
      continue;
    }
    for (const item of scanNow()) residue.push(`session ${n}: ${item}`);
  }
  return { sessions: sessions - failed.length, residue: [...residue, ...failed] };
}

export function main(argv) {
  const all = (name) =>
    argv.flatMap((a, i) => (a === name && argv[i + 1] !== undefined ? [argv[i + 1]] : []));
  const flag = (name, fallback) => all(name)[0] ?? fallback;
  const profile = flag("--profile");
  const steam = flag("--steam");
  if (!profile) {
    console.error(
      "usage: launcher-residue.mjs --profile <dir> [--steam <dir>] [--once | --sessions n --login-cmd c --end-cmd c ...] [--out file]",
    );
    return 2;
  }
  const ignore = all("--ignore-credential").map((s) => new RegExp(s, "i"));
  const scanNow = () => [...scan({ profile, steam }), ...savedCredentials(ignore)];
  if (argv.includes("--once")) {
    const found = scanNow();
    for (const f of found) console.error(`residue: ${f}`);
    return found.length ? 1 : 0;
  }
  const loginCmd = flag("--login-cmd");
  const endCmds = all("--end-cmd");
  if (!loginCmd || !endCmds.length) {
    console.error("launcher-residue: a run needs --login-cmd and at least one --end-cmd");
    return 2;
  }
  const sessions = Number(flag("--sessions", 20));
  const result = {
    os: flag("--os", "Windows"),
    ...drive({ sessions, loginCmd, endCmds, scanNow }),
  };
  const out = flag("--out");
  if (out) {
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, JSON.stringify(result, null, 2));
  }
  for (const r of result.residue) console.error(`residue: ${r}`);
  console.log(`launcher-residue: ${result.sessions} sessions, ${result.residue.length} residual`);
  return result.residue.length ? 1 : 0;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
