import { expect, test } from "@playwright/test";
import { gstinCheckChar, normalizeGstin, validGstin } from "../src/lib/gstin";

const VALID = [
  "27AAPFU0939F1ZV",
  "29AAGCB7383J1Z4",
  "24AAACC1206D1ZM",
  "27AAACR5055K1Z7",
  "09AAACH7409R1ZZ",
];

test("the five known GSTINs pass and their check characters match", () => {
  for (const g of VALID) {
    expect(validGstin(g), g).toBe(true);
    expect(gstinCheckChar(g.slice(0, 14)), g).toBe(g[14]);
  }
});

test("a wrong check character fails", () => {
  for (const g of VALID) {
    const wrong = g.slice(0, 14) + (g[14] === "A" ? "B" : "A");
    expect(validGstin(wrong), wrong).toBe(false);
  }
});

test("the shape is checked", () => {
  for (const bad of [
    "",
    "00AAPFU0939F1ZV",
    "39AAPFU0939F1ZV",
    "27AAPFU0939F1YV",
    "27AAPFU0939F1Z",
    "27AAPFU0939F1ZVX",
    "27AAPF10939F1ZV",
    "27AAPFU0939F0ZV",
    "27aapfu0939f1zv",
  ]) {
    expect(validGstin(bad), bad).toBe(false);
  }
  expect(validGstin(normalizeGstin(" 27aapfu0939f1zv "))).toBe(true);
});
