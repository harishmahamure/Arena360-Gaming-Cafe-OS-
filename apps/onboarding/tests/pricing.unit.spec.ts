import { expect, test } from "@playwright/test";
import { GST_PERCENT, PRICING, planPrice, priceFor } from "../../../packages/ui/src/pricing";

test("the pricing table pins every plan's price, GST and feature lines", () => {
  expect(PRICING.map((p) => p.key)).toEqual(["free", "basic", "premium"]);
  expect(GST_PERCENT).toBe(18);
  const free = planPrice("free");
  expect([free.currency, free.monthly, free.yearly, free.gstPercent, free.perBranch]).toEqual([
    "INR",
    0,
    0,
    0,
    false,
  ]);
  expect(free.features).toEqual(["pricing.free.branches", "pricing.free.ads"]);
  const basic = planPrice("basic");
  expect([basic.currency, basic.monthly, basic.yearly, basic.gstPercent, basic.perBranch]).toEqual([
    "INR",
    200,
    2000,
    18,
    true,
  ]);
  const premium = planPrice("premium");
  expect([
    premium.currency,
    premium.monthly,
    premium.yearly,
    premium.gstPercent,
    premium.perBranch,
  ]).toEqual(["INR", 1999, 19990, 18, false]);
  expect(priceFor(basic, "monthly")).toBe(200);
  expect(priceFor(premium, "yearly")).toBe(19990);
});
