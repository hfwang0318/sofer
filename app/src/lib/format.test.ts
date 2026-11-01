import { describe, expect, it } from "vitest";
import { formatBytes, formatEta, formatPlatform, formatSpeed } from "./format";

describe("formatBytes", () => {
  it("renders zero and invalid sizes as 0 MB", () => {
    expect(formatBytes(0)).toBe("0 MB");
    expect(formatBytes(-10)).toBe("0 MB");
    expect(formatBytes(Number.NaN)).toBe("0 MB");
  });

  it("picks B, KB, MB and GB units", () => {
    expect(formatBytes(999)).toBe("999 B");
    expect(formatBytes(1_500)).toBe("2 KB");
    expect(formatBytes(1_500_000)).toBe("2 MB");
    expect(formatBytes(1_434_837_056)).toBe("1.43 GB");
  });
});

describe("formatSpeed", () => {
  it("renders an unknown speed as a dash", () => {
    expect(formatSpeed(0)).toBe("—");
  });

  it("renders megabytes per second with one decimal", () => {
    expect(formatSpeed(1_240_000)).toBe("1.2 MB/s");
  });
});

describe("formatEta", () => {
  it("returns nothing when the ETA is unknown", () => {
    expect(formatEta(null)).toBe("");
  });

  it("marks a finished download and counts short seconds", () => {
    expect(formatEta(0)).toBe("即将完成");
    expect(formatEta(30)).toBe("剩余 30 秒");
  });

  it("splits longer waits into minutes and padded seconds", () => {
    expect(formatEta(65)).toBe("剩余 1 分 05 秒");
    expect(formatEta(90)).toBe("剩余 1 分 30 秒");
  });
});

describe("formatPlatform", () => {
  it("turns runtime triples into product names", () => {
    expect(formatPlatform("macos-aarch64")).toBe("macOS · Apple 芯片");
    expect(formatPlatform("macos-x86_64")).toBe("macOS · Intel");
    expect(formatPlatform("windows-x86_64")).toBe("Windows · x64");
    expect(formatPlatform("windows-aarch64")).toBe("Windows · ARM64");
  });

  it("leaves unknown triples untouched", () => {
    expect(formatPlatform("linux-x86_64")).toBe("linux-x86_64");
  });
});
