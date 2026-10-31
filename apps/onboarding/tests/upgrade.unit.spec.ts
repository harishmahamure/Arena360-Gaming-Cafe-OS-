import { expect, test } from "@playwright/test";
import { hasSubscription, lockedTier, returnTarget, subscriptionUrl } from "../src/lib/upgrade";

const OWNER = "https://app.arena360.cloud";
const OWN = "https://start.arena360.cloud";

test("the return link is followed only on the Owner app or this app", () => {
  expect(returnTarget(`${OWNER}/reports?x=1`, OWNER, OWN)).toBe(`${OWNER}/reports?x=1`);
  expect(returnTarget(`${OWN}/setup/devices?branch=b1`, OWNER, OWN)).toBe(
    `${OWN}/setup/devices?branch=b1`,
  );
  expect(returnTarget("/setup/staff", OWNER, OWN)).toBe(`${OWN}/setup/staff`);
});

test("anything else lands on the Owner app's home", () => {
  for (const raw of [
    null,
    "",
    "https://evil.example/x",
    "javascript:alert(1)",
    "//evil.example/x",
    "https://app.arena360.cloud.evil.example/",
  ]) {
    expect(returnTarget(raw, OWNER, OWN)).toBe(`${OWNER}/`);
  }
});

test("only basic and premium are bought", () => {
  expect(lockedTier("basic")).toBe("basic");
  expect(lockedTier("premium")).toBe("premium");
  expect(lockedTier("free")).toBeNull();
  expect(lockedTier(null)).toBeNull();
});

test("a live subscription is managed, an unpaid or over one is not", () => {
  for (const state of [
    "active",
    "grace",
    "lapsed",
    "renewal_needs_approval",
    "pending_review",
  ] as const) {
    expect(hasSubscription(state)).toBe(true);
  }
  for (const state of ["pending", "abandoned", "ended", "refunded"] as const) {
    expect(hasSubscription(state)).toBe(false);
  }
});

test("Account -> Subscription opens on the branch", () => {
  expect(subscriptionUrl(`${OWNER}/`, "b 1")).toBe(`${OWNER}/account/subscription?branch=b%201`);
});
