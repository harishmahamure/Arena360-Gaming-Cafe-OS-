import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { validEmail, validPhone, validText } from "../src/lib/validate";

// The form rules against the vectors the cloud's tests read too (no browser page is used).

type Vectors = {
  text: [string, boolean][];
  text_lengths: [number, boolean][];
  phone: [string, boolean][];
  email: [string, boolean][];
};

const vectors: Vectors = JSON.parse(
  readFileSync(
    fileURLToPath(new URL("../../../tests/fixtures/signup-validation.json", import.meta.url)),
    "utf8",
  ),
);

test("text fields follow the shared vectors", () => {
  for (const [v, ok] of vectors.text) expect(validText(v), JSON.stringify(v)).toBe(ok);
  for (const [n, ok] of vectors.text_lengths) expect(validText("x".repeat(n)), String(n)).toBe(ok);
});

test("phones follow the shared vectors", () => {
  for (const [v, ok] of vectors.phone) expect(validPhone(v), v).toBe(ok);
});

test("emails follow the shared vectors", () => {
  for (const [v, ok] of vectors.email) expect(validEmail(v), v).toBe(ok);
});
