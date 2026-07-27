import { beforeEach, describe, expect, test, vi } from "vitest";

const { mockCreateRomanizer, mockRomanizeLines } = vi.hoisted(() => ({
  mockCreateRomanizer: vi.fn(),
  mockRomanizeLines: vi.fn(),
}));

vi.mock("lyric-romanizer", () => ({
  createRomanizer: mockCreateRomanizer,
}));

describe("romanizeLyricsLines", () => {
  beforeEach(() => {
    vi.resetModules();
    mockCreateRomanizer.mockReset();
    mockRomanizeLines.mockReset();
    mockCreateRomanizer.mockReturnValue({
      romanizeLines: mockRomanizeLines,
    });
    mockRomanizeLines.mockResolvedValue({
      script: "chinese",
      lines: ["ni hao"],
    });
  });

  test("keeps Latin lyrics on the detector path without loading the full romanizer", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    const { result, requestId } = await romanizeLyricsLines(["Hello world"]);
    expect(result).toEqual(["Hello world"]);
    expect(requestId).toBe(-1);

    expect(mockCreateRomanizer).not.toHaveBeenCalled();
  });

  test("loads and reuses the full romanizer for non-Latin lyrics", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    const { result } = await romanizeLyricsLines(["你好"]);
    expect(result).toEqual(["ni hao"]);
    await romanizeLyricsLines(["世界"]);

    expect(mockCreateRomanizer).toHaveBeenCalledTimes(1);
    expect(mockRomanizeLines).toHaveBeenCalledTimes(2);
  });

  test("pins the cantonese dialect when language is cantonese", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    await romanizeLyricsLines(["你好"], "cantonese");

    expect(mockRomanizeLines).toHaveBeenCalledWith(["你好"], {
      script: "chinese",
      dialect: "cantonese",
    });
  });

  test("pins the japanese script when language is japanese", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    await romanizeLyricsLines(["恋愛"], "japanese");

    expect(mockRomanizeLines).toHaveBeenCalledWith(["恋愛"], {
      script: "japanese",
    });
  });

  test("passes the whole array in one call when language is unknown", async () => {
    mockRomanizeLines.mockImplementation(async (lines: readonly string[]) => ({
      script: "chinese",
      lines: lines.map((line) => (line === "你好" ? "ni hao" : line)),
    }));
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    const { result } = await romanizeLyricsLines(["Hello", "你好", "World"]);

    expect(result).toEqual(["Hello", "ni hao", "World"]);
    expect(mockRomanizeLines).toHaveBeenCalledTimes(1);
    expect(mockRomanizeLines).toHaveBeenCalledWith(["Hello", "你好", "World"]);
  });

  test("returns monotonically increasing requestIds for non-Latin content", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    const { requestId: id1 } = await romanizeLyricsLines(["你好"]);
    const { requestId: id2 } = await romanizeLyricsLines(["世界"]);

    expect(id1).toBeGreaterThan(0);
    expect(id2).toBeGreaterThan(id1);
  });
});
