import { describe, expect, it } from "vitest";
import { pageCount, pageOfIndex } from "./tablePage";

describe("table paging", () => {
  it("always has at least one page", () => {
    expect(pageCount(0, 100)).toBe(1);
  });

  it("only starts a new page once a page is full", () => {
    expect(pageCount(100, 100)).toBe(1);
    expect(pageCount(101, 100)).toBe(2);
  });

  it("finds the page holding a row", () => {
    expect(pageOfIndex(0, 100)).toBe(1);
    expect(pageOfIndex(99, 100)).toBe(1);
    expect(pageOfIndex(100, 100)).toBe(2);
  });
});
