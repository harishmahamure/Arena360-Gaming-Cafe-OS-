import { test } from "node:test";
import assert from "node:assert/strict";
import {
  claimVideo,
  isActiveVideo,
  isHubMediaUrl,
  mediaStage,
  onVideoClaims,
} from "../packages/ui/src/game-media.ts";

const SHA = "a".repeat(64);
const V = `/media/${SHA}`;
const C = `/media/${"b".repeat(64)}`;
const I = `/media/${"c".repeat(64)}`;

test("only Hub media URLs are accepted", () => {
  assert.equal(isHubMediaUrl(V), true);
  assert.equal(isHubMediaUrl(`https://hub.lan:7401/media/${SHA}`), true);
  assert.equal(isHubMediaUrl(`https://192.168.1.2:7401/media/${SHA}`), true);
  for (const bad of [
    null,
    undefined,
    "",
    "https://cdn.example.com/video.mp4",
    `https://cdn.example.com/media/${SHA}`,
    `https://hub.lan:7402/media/${SHA}`,
    `https://hub.lan/media/${SHA}`,
    `//hub.lan:7401/media/${SHA}`,
    `ftp://hub.lan:7401/media/${SHA}`,
    `/media/${SHA}?x=1`,
    `/media/${SHA}#x`,
    `/media/${"a".repeat(63)}`,
    `/media/${"A".repeat(64)}`,
    `/other/${SHA}`,
    `data:video/webm;base64,AAAA`,
    `blob:https://hub.lan:7401/${SHA}`,
  ]) {
    assert.equal(isHubMediaUrl(bad), false, String(bad));
  }
});

test("the chain runs video → cover → icon → solid", () => {
  assert.equal(mediaStage({ video: V, cover: C, icon: I }), "video");
  assert.equal(mediaStage({ cover: C, icon: I }), "cover");
  assert.equal(mediaStage({ icon: I }), "icon");
  assert.equal(mediaStage({}), "solid");
});

test("a failed step falls straight to the next", () => {
  assert.equal(mediaStage({ video: V, cover: C, icon: I, failed: [V] }), "cover");
  assert.equal(mediaStage({ video: V, cover: C, icon: I, failed: [V, C] }), "icon");
  assert.equal(mediaStage({ video: V, cover: C, icon: I, failed: [V, C, I] }), "solid");
});

test("a URL that is not Hub media is skipped, never loaded", () => {
  assert.equal(mediaStage({ video: "https://cdn.example.com/a.mp4", cover: C }), "cover");
  assert.equal(mediaStage({ video: "https://cdn.example.com/a.mp4", cover: "x" }), "solid");
});

test("a running game drops the video but keeps the cover", () => {
  assert.equal(mediaStage({ video: V, cover: C, gameRunning: true }), "cover");
  assert.equal(mediaStage({ video: V, gameRunning: true }), "solid");
});

test("high contrast skips straight to solid", () => {
  assert.equal(mediaStage({ video: V, cover: C, icon: I, highContrast: true }), "solid");
});

test("one video plays at a time: the newest claim wins and the older resumes on release", () => {
  let calls = 0;
  const off = onVideoClaims(() => calls++);
  const releaseA = claimVideo("a");
  assert.equal(isActiveVideo("a"), true);
  const releaseB = claimVideo("b");
  assert.equal(isActiveVideo("a"), false);
  assert.equal(isActiveVideo("b"), true);
  releaseB();
  assert.equal(isActiveVideo("a"), true);
  releaseA();
  assert.equal(isActiveVideo("a"), false);
  assert.equal(calls, 4);
  off();
  claimVideo("c")();
  assert.equal(calls, 4);
});
