import { expect, test } from "@playwright/test";
import { buyerErrors, gstinValid, normaliseGstin, toBuyer } from "../src/lib/business";

test("a GSTIN needs the shape, a state code and the check character", () => {
  expect(gstinValid("27AAPFU0939F1ZV")).toBe(true);
  expect(gstinValid("29AABCT1332L1ZA")).toBe(true);
  for (const bad of [
    "27AAPFU0939F1ZW",
    "00AAPFU0939F1ZV",
    "27aapfu0939f1zv",
    "27AAPFU0939F1Z",
    "27AAPFU0939F1XV",
    "",
  ]) {
    expect(gstinValid(bad), bad).toBe(false);
  }
});

test("the GSTIN text is trimmed and upper-cased before it is checked and sent", () => {
  expect(normaliseGstin("  29aabct1332l1za ")).toBe("29AABCT1332L1ZA");
  expect(buyerErrors(" 29aabct1332l1za ", "Acme")).toEqual({});
  expect(toBuyer(true, " 29aabct1332l1za ", " Acme ")).toEqual({
    gstin: "29AABCT1332L1ZA",
    legal_name: "Acme",
  });
  expect(toBuyer(false, "29AABCT1332L1ZA", "Acme")).toBeUndefined();
});

test("each Business invoice field reports its own error", () => {
  expect(buyerErrors("", "Acme")).toEqual({ gstin: "hub.settle.business.err.gstin" });
  expect(buyerErrors("29AABCT1332L1ZA", "  ")).toEqual({
    legal_name: "hub.settle.business.err.name",
  });
  expect(buyerErrors("29AABCT1332L1ZA", "x".repeat(101))).toEqual({
    legal_name: "hub.settle.business.err.name",
  });
  expect(Object.keys(buyerErrors("bad", ""))).toEqual(["gstin", "legal_name"]);
});
