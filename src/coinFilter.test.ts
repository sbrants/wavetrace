import { describe, expect, it } from "vitest";
import { formatCoin, parseOptionalCoin } from "./api";

describe("parseOptionalCoin", () => {
  it("blank is null", () => {
    expect(parseOptionalCoin("")).toBeNull();
    expect(parseOptionalCoin("   ")).toBeNull();
  });

  it("plain numbers still work", () => {
    expect(parseOptionalCoin("600000000000000")).toBe(6e14);
    expect(parseOptionalCoin("1,000,000")).toBe(1e6);
  });

  it("parses game-style suffixes", () => {
    expect(parseOptionalCoin("600T")).toBe(600e12);
    expect(parseOptionalCoin("1.5q")).toBe(1.5e15);
    expect(parseOptionalCoin("2Q")).toBe(2e18);
    expect(parseOptionalCoin("3K")).toBe(3e3);
  });

  it("is case-sensitive between q (1e15) and Q (1e18), matching the game", () => {
    expect(parseOptionalCoin("1q")).toBe(1e15);
    expect(parseOptionalCoin("1Q")).toBe(1e18);
    expect(parseOptionalCoin("1s")).toBe(1e21);
    expect(parseOptionalCoin("1S")).toBe(1e24);
  });

  it("tolerates whitespace between the number and suffix", () => {
    expect(parseOptionalCoin(" 600 T ")).toBe(600e12);
  });

  it("rejects unknown suffixes and garbage", () => {
    expect(parseOptionalCoin("600X")).toBe(false);
    expect(parseOptionalCoin("abc")).toBe(false);
    expect(parseOptionalCoin("600T/min")).toBe(false);
  });

  it("round-trips through formatCoin for whole-suffix values", () => {
    for (const v of [600e12, 1.5e15, 2e18, 998e12]) {
      expect(parseOptionalCoin(formatCoin(v))).toBe(v);
    }
  });
});
