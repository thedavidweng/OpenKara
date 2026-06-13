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

  test("passes cantonese dialect option when language is cantonese", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    await romanizeLyricsLines(["你好"], "cantonese");

    expect(mockRomanizeLines).toHaveBeenCalledWith(["你好"], {
      script: "chinese",
      dialect: "cantonese",
    });
  });

  test("does not pass dialect options for non-cantonese languages", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    await romanizeLyricsLines(["你好"], "mandarin");

    expect(mockRomanizeLines).toHaveBeenCalledWith(["你好"], undefined);
  });

  test("keeps Latin lines unchanged and romanizes only non-Latin lines in mixed content", async () => {
    mockRomanizeLines.mockResolvedValue({
      script: "chinese",
      lines: ["ni hao"],
    });
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    const { result } = await romanizeLyricsLines(["Hello", "你好", "World"]);

    expect(result).toEqual(["Hello", "ni hao", "World"]);
    // romanizeLines should only be called for the non-Latin line
    expect(mockRomanizeLines).toHaveBeenCalledTimes(1);
    expect(mockRomanizeLines).toHaveBeenCalledWith(["你好"], undefined);
  });

  test("returns monotonically increasing requestIds for non-Latin content", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    const { requestId: id1 } = await romanizeLyricsLines(["你好"]);
    const { requestId: id2 } = await romanizeLyricsLines(["世界"]);

    expect(id1).toBeGreaterThan(0);
    expect(id2).toBeGreaterThan(id1);
  });
});
