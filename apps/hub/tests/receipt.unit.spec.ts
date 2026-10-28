import { expect, test } from "@playwright/test";
import { isPaidTier, lineWidth, previewReceipt, wrapLine, wrapText } from "../src/lib/receipt";

test("a line wraps at word boundaries within the paper width", () => {
  expect(wrapLine("Thank you for playing at Arena 360 today", 32)).toEqual([
    "Thank you for playing at Arena",
    "360 today",
  ]);
  expect(wrapLine("exactly thirty two characters!!!", 32)).toEqual([
    "exactly thirty two characters!!!",
  ]);
});

test("a word longer than the line is cut at the width", () => {
  expect(wrapLine("x".repeat(70), 32)).toEqual(["x".repeat(32), "x".repeat(32), "x".repeat(6)]);
  expect(wrapLine(`hi ${"y".repeat(33)}`, 32)).toEqual(["hi", "y".repeat(32), "y"]);
});

test("each text line wraps on its own and blank lines are dropped", () => {
  expect(wrapText("a\n\nb c", 48)).toEqual(["a", "b c"]);
  expect(wrapText("", 48)).toEqual([]);
});

test("58 mm is 32 characters wide and 80 mm is 48", () => {
  expect(lineWidth("58")).toBe(32);
  expect(lineWidth("80")).toBe(48);
});

test("the preview centres the header and footer and wraps each at the width", () => {
  const input = {
    name: "Arena 360 Pune",
    address: "Shop 4, Some Very Long Road Name, Kothrud, Pune 411038",
    phone: "+91 98765 43210",
    gstin: "27ABCDE1234F1Z5",
    footer: "Thanks!",
    body: "[items]",
  };
  const narrow = previewReceipt({ ...input, paperMm: "58" });
  expect(narrow.every((l) => l.length <= 32)).toBe(true);
  expect(narrow).toContain("-".repeat(32));
  expect(narrow.some((l) => l.includes("GSTIN 27ABCDE1234F1Z5"))).toBe(true);
  const wide = previewReceipt({ ...input, paperMm: "80", gstin: "" });
  expect(wide.every((l) => l.length <= 48)).toBe(true);
  expect(wide.some((l) => l.includes("GSTIN"))).toBe(false);
  expect(wide.at(-1)?.trim()).toBe("Thanks!");
  expect(wide.length).toBeLessThan(narrow.length);
});

test("only Basic and Premium are paid", () => {
  expect(isPaidTier("basic") && isPaidTier("premium")).toBe(true);
  expect(isPaidTier("free") || isPaidTier(undefined) || isPaidTier("")).toBe(false);
});
