import { describe, expect, test } from "vitest";
import { hanPinyinInitial } from "./han-pinyin-initials";

describe("hanPinyinInitial", () => {
  test("returns the default first letter for common Han characters", () => {
    expect(hanPinyinInitial("北".codePointAt(0) ?? 0)).toBe("B");
    expect(hanPinyinInitial("苹".codePointAt(0) ?? 0)).toBe("P");
    expect(hanPinyinInitial("中".codePointAt(0) ?? 0)).toBe("Z");
  });

  test("returns null for unmapped or non-Han code points", () => {
    expect(hanPinyinInitial(0x41)).toBeNull();
    expect(hanPinyinInitial(0x20000)).toBeNull();
    expect(hanPinyinInitial(0x3400)).toBeNull();
    expect(hanPinyinInitial(0xf900)).toBeNull();
  });
});
