import { expect, test } from "@playwright/test";
import {
  PROGRAMS_MAX,
  closeText,
  parsePrograms,
  seconds,
  storedValue,
  validField,
} from "../src/lib/booster";

test("the close list takes executable names on lines or after commas, trimmed", () => {
  expect(parsePrograms("")).toEqual({ programs: [] });
  expect(parsePrograms("  \n ")).toEqual({ programs: [] });
  expect(parsePrograms("discord.exe\n OneDrive.exe ,chrome.exe")).toEqual({
    programs: ["discord.exe", "OneDrive.exe", "chrome.exe"],
  });
  expect(storedValue("close", "discord.exe\nchrome.exe")).toBe("discord.exe,chrome.exe");
  expect(closeText("discord.exe, chrome.exe")).toBe("discord.exe\nchrome.exe");
  expect(closeText("")).toBe("");
});

test("anything that is not an .exe file name is refused, and so is a repeat or too many", () => {
  for (const bad of [
    "discord",
    "C:\\x\\discord.exe",
    "a/b.exe",
    ".exe",
    "a;b.exe",
    "a".repeat(70) + ".exe",
  ]) {
    expect(parsePrograms(bad), bad).toEqual({ problem: "shape" });
  }
  expect(parsePrograms("a.exe\nA.EXE")).toEqual({ problem: "duplicate" });
  const many = Array.from({ length: PROGRAMS_MAX + 1 }, (_, i) => `p${i}.exe`).join("\n");
  expect(parsePrograms(many)).toEqual({ problem: "too_many" });
  expect(validField("close", "discord")).toBe(false);
  expect(validField("close", "discord.exe")).toBe(true);
});

test("priority and power take only their words", () => {
  for (const ok of ["normal", "above_normal", "high"])
    expect(validField("priority", ok)).toBe(true);
  expect(validField("priority", "realtime")).toBe(false);
  expect(validField("power", "true") && validField("power", "false")).toBe(true);
  expect(validField("power", "yes")).toBe(false);
});

test("a launch time shows as seconds with one decimal", () => {
  expect(seconds(14_200)).toBe("14.2");
  expect(seconds(950)).toBe("1.0");
  expect(seconds(0)).toBe("0.0");
  expect(seconds(180_000)).toBe("180.0");
  expect(seconds(14_249)).toBe("14.2");
  expect(seconds(14_250)).toBe("14.3");
});
