#!/usr/bin/env node
// Checks `strace -f -e trace=connect` logs from the WAN-down job (Story 1.22): every connect()
// must go to loopback or a local socket family, except the connectivity probe's own endpoints
// (passed with --allow ip:port), which are allowed only in the Hub's trace (`trace.hub.log`,
// the probe's process): the kiosk, the driver or the UI connecting to them is a violation.
// The Hub UI smoke's trace (`trace.ui.log`) may also make Chromium's built-in IPv6
// reachability check (CHROMIUM_IPV6_PROBE), and only that. Anything else is a violation and
// the script exits 1.
//
// usage: node scripts/wan-down/check-trace.mjs [--allow <ip:port>]... <trace.log>...

import { readFileSync } from "node:fs";
import { basename } from "node:path";
import { fileURLToPath } from "node:url";

/** Socket families that never leave the machine. */
const LOCAL_FAMILIES = new Set(["AF_UNIX", "AF_LOCAL", "AF_NETLINK", "AF_UNSPEC"]);

/**
 * The destination of one strace `connect(...)` line: `{ family, addr, port }`, `{ family }` for a
 * non-IP family, `null` for a line that is not a connect, or `{ family: "?" }` when the
 * sockaddr cannot be read.
 */
export function parseConnect(line) {
  if (!/\bconnect\(/.test(line) || /<\.\.\. connect resumed>/.test(line)) return null;
  const family = /sa_family=(AF_[A-Z0-9_]+)/.exec(line)?.[1];
  if (!family) return { family: "?" };
  if (family === "AF_INET") {
    const addr = /sin_addr=inet_addr\("([^"]+)"\)/.exec(line)?.[1];
    const port = /sin_port=htons\((\d+)\)/.exec(line)?.[1];
    return addr && port ? { family, addr, port: Number(port) } : { family: "?" };
  }
  if (family === "AF_INET6") {
    const addr = /inet_pton\(AF_INET6,\s*"([^"]+)"/.exec(line)?.[1];
    const port = /sin6_port=htons\((\d+)\)/.exec(line)?.[1];
    return addr && port
      ? { family, addr: addr.toLowerCase(), port: Number(port) }
      : { family: "?" };
  }
  return { family };
}

/** Whether an IP address is loopback (127/8, ::1, or an IPv4-mapped 127/8). */
export function isLoopback(addr) {
  const v4 = addr.startsWith("::ffff:") ? addr.slice(7) : addr;
  if (/^127\.\d+\.\d+\.\d+$/.test(v4)) return true;
  return addr === "::1" || addr === "0:0:0:0:0:0:0:1";
}

/** The allow-list key of an address and port: `1.1.1.1:443` or `[2606:4700::1111]:443`. */
export function endpointKey(addr, port) {
  return addr.includes(":") ? `[${addr}]:${port}` : `${addr}:${port}`;
}

/** Normalises an `--allow` value the same way (`[v6]` lower-cased). */
export function normaliseAllow(value) {
  const m = /^\[([^\]]+)\]:(\d+)$/.exec(value) ?? /^([^:]+):(\d+)$/.exec(value);
  if (!m) throw new Error(`invalid --allow ${JSON.stringify(value)} (ip:port)`);
  return endpointKey(m[1].toLowerCase(), Number(m[2]));
}

/** The trace the probe runs in: only it may connect to the probe's endpoints. */
export const PROBE_TRACE = "trace.hub.log";

/** The trace of the Hub UI smoke (vite preview, Playwright and the browser). */
export const UI_TRACE = "trace.ui.log";

/**
 * Chromium's built-in IPv6 reachability check: before its first navigation the network
 * service connects a UDP socket to this public address to learn whether IPv6 is routable.
 * In the namespace it fails with ENETUNREACH and, being a UDP connect, sends nothing. No
 * launch flag turns it off (tried in the Story 1.22 review), so it is allowed in the UI trace
 * only.
 */
export const CHROMIUM_IPV6_PROBE = "[2001:4860:4860::8888]:443";

/**
 * The allow-list for the trace `file`: `allow` (the probe's endpoints) for the Hub's trace,
 * only CHROMIUM_IPV6_PROBE for the UI's, and nothing for any other.
 */
export function allowFor(file, allow) {
  const name = basename(file);
  if (name === PROBE_TRACE) return allow;
  if (name === UI_TRACE) return new Set([CHROMIUM_IPV6_PROBE]);
  return new Set();
}

/** The lines of `text` whose connect() is neither local nor allowed. */
export function violations(text, allow) {
  const bad = [];
  for (const line of text.split("\n")) {
    const c = parseConnect(line);
    if (!c) continue;
    if (LOCAL_FAMILIES.has(c.family)) continue;
    if (c.family === "AF_INET" || c.family === "AF_INET6") {
      if (isLoopback(c.addr) || allow.has(endpointKey(c.addr, c.port))) continue;
    }
    bad.push(line.trim());
  }
  return bad;
}

function main(argv) {
  const allow = new Set();
  const files = [];
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--allow") allow.add(normaliseAllow(argv[++i] ?? ""));
    else files.push(argv[i]);
  }
  if (files.length === 0) {
    console.error("usage: check-trace.mjs [--allow <ip:port>]... <trace.log>...");
    return 64;
  }
  let total = 0;
  let connects = 0;
  for (const file of files) {
    const text = readFileSync(file, "utf8");
    connects += text.split("\n").filter((l) => parseConnect(l)).length;
    for (const line of violations(text, allowFor(file, allow))) {
      console.error(`${file}: outbound connect outside loopback and the probe: ${line}`);
      total++;
    }
  }
  if (total > 0) {
    console.error(`check-trace: ${total} violation(s)`);
    return 1;
  }
  console.log(
    `check-trace: ${connects} connect() calls in ${files.length} trace(s), all loopback, local, the probe in ${PROBE_TRACE} (${[...allow].join(", ") || "none allowed"}) or Chromium's IPv6 check in ${UI_TRACE}`,
  );
  return 0;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  process.exit(main(process.argv.slice(2)));
}
