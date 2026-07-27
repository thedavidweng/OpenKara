import { describe, expect, test } from "vitest";
import { splitCompanionRomanization } from "./lyrics-companion-romanization";
import type { LyricLine } from "@/types/ipc";

function line(
  timeMs: number,
  text: string,
  words: LyricLine["words"] = [],
): LyricLine {
  return { time_ms: timeMs, text, words, bg_words: null, section: null };
}

describe("splitCompanionRomanization", () => {
  test("lifts same-timestamp romaji shadows out of the lyric list", () => {
    // Verbatim shape of the embedded LRC for imase - NIGHT DANCER.
    const split = splitCompanionRomanization([
      line(850, "どうでもいいような 夜だけど"),
      line(850, "doudemoiiyouna yorudakedo"),
      line(4850, "響めき 煌めきと君も"),
      line(4850, "kyoumeki koumekitokunmo"),
      line(25810, "まだ止まった 刻む針も"),
      line(25810, "madatomatta kizamuharimo"),
      line(29070, "入り浸った 散らかる部屋も"),
      line(29070, "irihitatsuta chirakaruheyamo"),
    ]);

    expect(split.lines.map((l) => l.text)).toEqual([
      "どうでもいいような 夜だけど",
      "響めき 煌めきと君も",
      "まだ止まった 刻む針も",
      "入り浸った 散らかる部屋も",
    ]);
    expect(split.romanizedLines).toEqual([
      "doudemoiiyouna yorudakedo",
      "kyoumeki koumekitokunmo",
      "madatomatta kizamuharimo",
      "irihitatsuta chirakaruheyamo",
    ]);
    expect(split.complete).toBe(true);
  });

  test("tolerates timestamps that differ only by rounding", () => {
    const split = splitCompanionRomanization([
      line(850, "どうでもいいような 夜だけど"),
      line(860, "doudemoiiyouna yorudakedo"),
      line(4850, "響めき 煌めきと君も"),
      line(4850, "kyoumeki koumekitokunmo"),
      line(25810, "まだ止まった 刻む針も"),
      line(25850, "madatomatta kizamuharimo"),
    ]);

    expect(split.lines).toHaveLength(3);
    expect(split.romanizedLines).toEqual([
      "doudemoiiyouna yorudakedo",
      "kyoumeki koumekitokunmo",
      "madatomatta kizamuharimo",
    ]);
    expect(split.complete).toBe(true);
  });

  test("keeps a transcription stamped with a different beat as a lyric line", () => {
    const split = splitCompanionRomanization([
      line(850, "どうでもいいような 夜だけど"),
      line(850, "doudemoiiyouna yorudakedo"),
      line(4850, "響めき 煌めきと君も"),
      line(4850, "kyoumeki koumekitokunmo"),
      line(25810, "まだ止まった 刻む針も"),
      line(25810, "madatomatta kizamuharimo"),
      line(57530, "無駄話で はぐらかして"),
      line(57530, "mudabanashide hagurakashite"),
      line(57530, "触れた先を ためらうように"),
      line(61510, "furetasakiwo tamerauyouni"),
    ]);

    expect(split.lines.map((l) => l.text)).toEqual([
      "どうでもいいような 夜だけど",
      "響めき 煌めきと君も",
      "まだ止まった 刻む針も",
      "無駄話で はぐらかして",
      "触れた先を ためらうように",
      "furetasakiwo tamerauyouni",
    ]);
    expect(split.complete).toBe(false);
  });

  test("never absorbs a genuine English lyric that follows an untranscribed line", () => {
    // REGRESSION: pairing by adjacency alone deleted this line — invisible
    // with the toggle off, misattributed as pronunciation with it on.
    const englishLyric = line(5000, "I love you");
    const split = splitCompanionRomanization([
      line(1000, "日本語のA"),
      line(1000, "nihongo no A"),
      line(2000, "日本語のB"),
      line(2000, "nihongo no B"),
      line(3000, "日本語のC"),
      line(3000, "nihongo no C"),
      line(4000, "日本語のD"),
      englishLyric,
      line(6000, "日本語のE"),
      line(6000, "nihongo no E"),
    ]);

    expect(split.lines).toContain(englishLyric);
    expect(split.romanizedLines).not.toContain("I love you");
    expect(split.complete).toBe(false);
  });

  test("leaves a monolingual file untouched", () => {
    const lines = [
      line(0, "君のこと"),
      line(1000, "思い出しては"),
      line(2000, "変わらないね"),
      line(3000, "二人 歳を重ねてた"),
    ];

    const split = splitCompanionRomanization(lines);

    expect(split.lines).toBe(lines);
    expect(split.romanizedLines).toEqual([]);
    expect(split.complete).toBe(false);
  });

  test("never swallows genuine English lines in a Japanese song", () => {
    const lines = [
      line(0, "どうでもいいような 夜だけど"),
      line(4000, "I don't wanna say goodbye"),
      line(8000, "響めき 煌めきと君も"),
      line(12000, "All the love and all the shine"),
      line(16000, "変わらないね"),
    ];

    const split = splitCompanionRomanization(lines);

    expect(split.lines).toBe(lines);
    expect(split.romanizedLines).toEqual([]);
  });

  test("keeps word-timed lines as lyrics even when they are Latin", () => {
    // A karaoke-timed Latin line owns word highlighting; collapsing it into an
    // annotation would silently drop that timing.
    const timed = line(850, "doudemoiiyouna yorudakedo", [
      { time_ms: 850, end_ms: 1200, text: "doudemoiiyouna" },
    ]);
    const lines = [
      line(0, "ライン一"),
      line(0, "rain ichi"),
      line(1000, "ライン二"),
      line(1000, "rain ni"),
      line(2000, "ライン三"),
      line(2000, "rain san"),
      line(850, "どうでもいいような 夜だけど"),
      timed,
    ];

    const split = splitCompanionRomanization(lines);

    expect(split.lines).toContain(timed);
    expect(split.complete).toBe(false);
  });

  test("reports an incomplete split when only some lines are transcribed", () => {
    const split = splitCompanionRomanization([
      line(0, "ライン一"),
      line(0, "rain ichi"),
      line(1000, "ライン二"),
      line(1000, "rain ni"),
      line(2000, "ライン三"),
      line(2000, "rain san"),
      line(3000, "ライン四"),
      line(4000, "ライン五"),
    ]);

    expect(split.lines.map((l) => l.text)).toEqual([
      "ライン一",
      "ライン二",
      "ライン三",
      "ライン四",
      "ライン五",
    ]);
    expect(split.romanizedLines).toEqual([
      "rain ichi",
      "rain ni",
      "rain san",
      "",
      "",
    ]);
    expect(split.complete).toBe(false);
  });

  test("preserves blank separator lines", () => {
    const split = splitCompanionRomanization([
      line(0, "ライン一"),
      line(0, "rain ichi"),
      line(1000, ""),
      line(2000, "ライン二"),
      line(2000, "rain ni"),
      line(3000, "ライン三"),
      line(3000, "rain san"),
    ]);

    expect(split.lines.map((l) => l.text)).toEqual([
      "ライン一",
      "",
      "ライン二",
      "ライン三",
    ]);
    expect(split.romanizedLines).toEqual([
      "rain ichi",
      "",
      "rain ni",
      "rain san",
    ]);
  });
});
