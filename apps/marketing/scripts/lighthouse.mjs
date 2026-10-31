// Lighthouse gate for Home: Lighthouse's mobile profile (a mid-range phone, throttled "slow 4G",
// 4x CPU slowdown) against the built site, median of three runs. Fails unless Largest Contentful
// Paint is under 2.5 s and the performance and accessibility scores are each at least 90.
// Run `pnpm build` first (the `lighthouse` script does). Chromium comes from Playwright.

import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";
import lighthouse from "lighthouse";
import { serve } from "../tests/static-server.mjs";

export const LCP_BUDGET_MS = 2500;
export const MIN_SCORE = 90;
const SITE_PORT = 5182;
const DEBUG_PORT = 9223;
const RUNS = 3;

export const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];

/** The failures of one set of medians against the budgets; empty when Home passes. */
export function failures({ lcp, performance, accessibility }) {
  const out = [];
  if (!(lcp < LCP_BUDGET_MS))
    out.push(`LCP ${Math.round(lcp)} ms is not under ${LCP_BUDGET_MS} ms`);
  if (!(performance >= MIN_SCORE)) out.push(`performance ${performance} is under ${MIN_SCORE}`);
  if (!(accessibility >= MIN_SCORE))
    out.push(`accessibility ${accessibility} is under ${MIN_SCORE}`);
  return out;
}

async function main() {
  const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
  const profile = mkdtempSync(join(tmpdir(), "arena-lighthouse-"));
  const server = await serve(join(root, "dist"), SITE_PORT);
  const chrome = spawn(
    chromium.executablePath(),
    [
      "--headless=new",
      "--no-sandbox",
      "--disable-gpu",
      `--remote-debugging-port=${DEBUG_PORT}`,
      `--user-data-dir=${profile}`,
      "about:blank",
    ],
    { stdio: "ignore" },
  );
  try {
    await new Promise((done) => setTimeout(done, 2000));
    const samples = [];
    for (let i = 0; i < RUNS; i++) {
      const run = await lighthouse(`http://127.0.0.1:${SITE_PORT}/`, {
        port: DEBUG_PORT,
        output: "json",
        logLevel: "error",
        onlyCategories: ["performance", "accessibility"],
      });
      const { audits, categories } = run.lhr;
      samples.push({
        lcp: audits["largest-contentful-paint"].numericValue,
        performance: Math.round(categories.performance.score * 100),
        accessibility: Math.round(categories.accessibility.score * 100),
      });
    }
    const result = {
      lcp: median(samples.map((s) => s.lcp)),
      performance: median(samples.map((s) => s.performance)),
      accessibility: median(samples.map((s) => s.accessibility)),
    };
    console.log(
      `Home on a throttled phone: LCP ${Math.round(result.lcp)} ms, performance ${result.performance}, accessibility ${result.accessibility} (median of ${RUNS})`,
    );
    const failed = failures(result);
    for (const line of failed) console.error(`lighthouse: ${line}`);
    process.exitCode = failed.length === 0 ? 0 : 1;
  } finally {
    chrome.kill();
    server.close();
    try {
      rmSync(profile, { recursive: true, force: true });
    } catch {
      // Chrome may still hold the profile for a moment; the temp folder is cleaned by the OS.
    }
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
