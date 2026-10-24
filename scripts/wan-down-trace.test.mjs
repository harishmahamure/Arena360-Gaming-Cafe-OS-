import { test } from "node:test";
import assert from "node:assert/strict";
import {
  CHROMIUM_IPV6_PROBE,
  allowFor,
  endpointKey,
  isLoopback,
  normaliseAllow,
  parseConnect,
  violations,
} from "./wan-down/check-trace.mjs";

// Lines as `strace -f -e trace=connect -o trace.log` writes them.
const V4_PROBE =
  '4242  connect(9, {sa_family=AF_INET, sin_port=htons(443), sin_addr=inet_addr("1.1.1.1")}, 16) = -1 ENETUNREACH (Network is unreachable)';
const V4_LOOP =
  '4243  connect(10, {sa_family=AF_INET, sin_port=htons(7400), sin_addr=inet_addr("127.0.0.1")}, 16) = -1 EINPROGRESS (Operation now in progress)';
const V4_OUT =
  '4244  connect(11, {sa_family=AF_INET, sin_port=htons(443), sin_addr=inet_addr("142.250.1.1")}, 16) = -1 ENETUNREACH (Network is unreachable)';
const V6_LOOP =
  '4245  connect(12, {sa_family=AF_INET6, sin6_port=htons(7409), sin6_flowinfo=htonl(0), inet_pton(AF_INET6, "::1", &sin6_addr), sin6_scope_id=0}, 28) = 0';
const V6_OUT =
  '4246  connect(13, {sa_family=AF_INET6, sin6_port=htons(443), sin6_flowinfo=htonl(0), inet_pton(AF_INET6, "2606:4700::1111", &sin6_addr), sin6_scope_id=0}, 28) = -1 ENETUNREACH';
const UNIX = '4247  connect(14, {sa_family=AF_UNIX, sun_path="/tmp/k/ipc/kiosk.sock"}, 110) = 0';
const UNFINISHED =
  '4248  connect(15, {sa_family=AF_INET, sin_port=htons(80), sin_addr=inet_addr("93.184.216.34")}, 16 <unfinished ...>';
const RESUMED = "4248  <... connect resumed>) = -1 ENETUNREACH (Network is unreachable)";

test("parses IPv4, IPv6 and local connects; skips other lines", () => {
  assert.deepEqual(parseConnect(V4_PROBE), { family: "AF_INET", addr: "1.1.1.1", port: 443 });
  assert.deepEqual(parseConnect(V6_OUT), {
    family: "AF_INET6",
    addr: "2606:4700::1111",
    port: 443,
  });
  assert.deepEqual(parseConnect(UNIX), { family: "AF_UNIX" });
  assert.equal(parseConnect(RESUMED), null);
  assert.equal(parseConnect("4249  +++ exited with 0 +++"), null);
  assert.deepEqual(parseConnect("4250  connect(3, 0x7ffd, 16) = -1 EFAULT"), { family: "?" });
});

test("loopback and allow-list keys", () => {
  for (const a of ["127.0.0.1", "127.0.0.53", "::1", "::ffff:127.0.0.1"])
    assert.ok(isLoopback(a), a);
  for (const a of ["1.1.1.1", "::ffff:8.8.8.8", "10.0.0.1"]) assert.ok(!isLoopback(a), a);
  assert.equal(endpointKey("1.1.1.1", 443), "1.1.1.1:443");
  assert.equal(endpointKey("2606:4700::1111", 443), "[2606:4700::1111]:443");
  assert.equal(normaliseAllow("[2606:4700::ABCD]:443"), "[2606:4700::abcd]:443");
  assert.throws(() => normaliseAllow("1.1.1.1"));
});

test("only connects outside loopback, local families and the probe are violations", () => {
  const allow = new Set([normaliseAllow("1.1.1.1:443"), normaliseAllow("8.8.8.8:443")]);
  const text = [V4_PROBE, V4_LOOP, V6_LOOP, UNIX, RESUMED, V4_OUT, V6_OUT, UNFINISHED].join("\n");
  assert.deepEqual(violations(text, allow), [V4_OUT, V6_OUT, UNFINISHED]);
  // The probe's endpoint on another port is not the probe.
  assert.equal(violations(V4_PROBE.replace("htons(443)", "htons(80)"), allow).length, 1);
  // An unreadable sockaddr is never waved through.
  assert.equal(violations("1 connect(3, 0x7ffd, 16) = -1 EFAULT", allow).length, 1);
  assert.deepEqual(violations([V4_LOOP, UNIX].join("\n"), new Set()), []);
});

test("the probe's endpoints are allowed only in the Hub's trace", () => {
  const allow = new Set([normaliseAllow("1.1.1.1:443")]);
  assert.deepEqual(violations(V4_PROBE, allowFor("/out/down/trace.hub.log", allow)), []);
  assert.deepEqual(violations(V4_PROBE, allowFor("/out/down/trace.ui.log", allow)), [V4_PROBE]);
  for (const other of ["trace.kiosk.log", "/out/down/trace.driver.log"]) {
    assert.equal(allowFor(other, allow).size, 0, other);
    assert.deepEqual(violations(V4_PROBE, allowFor(other, allow)), [V4_PROBE], other);
    assert.deepEqual(violations(V4_LOOP, allowFor(other, allow)), [], other);
  }
});

test("Chromium's IPv6 reachability check is allowed in the UI trace only", () => {
  const chromium =
    '6030  connect(18, {sa_family=AF_INET6, sin6_port=htons(443), sin6_flowinfo=htonl(0), inet_pton(AF_INET6, "2001:4860:4860::8888", &sin6_addr), sin6_scope_id=0}, 28) = -1 ENETUNREACH (Network is unreachable)';
  assert.equal(CHROMIUM_IPV6_PROBE, "[2001:4860:4860::8888]:443");
  const probe = new Set([normaliseAllow("1.1.1.1:443")]);
  assert.deepEqual(violations(chromium, allowFor("/out/down/trace.ui.log", probe)), []);
  // The same address on another port is not the check.
  const other = chromium.replace("htons(443)", "htons(80)");
  assert.deepEqual(violations(other, allowFor("trace.ui.log", probe)), [other]);
  for (const file of ["trace.hub.log", "trace.kiosk.log", "trace.driver.log"]) {
    assert.deepEqual(violations(chromium, allowFor(file, probe)), [chromium], file);
  }
});
