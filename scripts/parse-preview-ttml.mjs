/**
 * Preview-catalog TTML parser. Mirrors the Rust `parse_ttml` contract enough
 * to embed Word-timed AMLL lyrics in `src/mock/preview-songs.ts`.
 */

export function parseTtmlTimestamp(value) {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  const match = trimmed.match(/^(?:(\d+):)?(\d+)(?:\.(\d+))?$/);
  if (!match) return null;
  const minutes = match[1] ? Number(match[1]) : 0;
  const seconds = Number(match[2]);
  if (!Number.isFinite(minutes) || !Number.isFinite(seconds) || seconds >= 60) {
    return null;
  }
  const frac = match[3] ?? "";
  let fractionMs = 0;
  if (frac.length === 1) fractionMs = Number(frac) * 100;
  else if (frac.length === 2) fractionMs = Number(frac) * 10;
  else if (frac.length >= 3) fractionMs = Number(frac.slice(0, 3));
  return minutes * 60_000 + seconds * 1_000 + fractionMs;
}

function localName(tag) {
  const stripped = tag.replace(/^<\/?/, "").replace(/\/?>$/, "");
  const name = stripped.split(/\s/)[0] ?? "";
  return name.includes(":") ? name.slice(name.lastIndexOf(":") + 1) : name;
}

function parseAttrs(openTag) {
  const attrs = {};
  const re = /([^\s=/:]+)="([^"]*)"/g;
  let match;
  while ((match = re.exec(openTag))) {
    const key = match[1].includes(":")
      ? match[1].slice(match[1].lastIndexOf(":") + 1)
      : match[1];
    attrs[key] = match[2];
  }
  return attrs;
}

function isPrettyPrintSpace(text) {
  return text.trim().length === 0 && /[\n\r]/.test(text);
}

function nonemptyTrimmed(text) {
  const trimmed = text.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function appendWordRoman(word, text) {
  if (!word) return;
  const piece = text.trim();
  if (!piece) return;
  word.roman = word.roman ? `${word.roman} ${piece}` : piece;
}

function joinedWordRomans(words) {
  if (words.length === 0) return null;
  const parts = [];
  for (const word of words) {
    const roman = word.roman?.trim();
    if (!roman) return null;
    parts.push(roman);
  }
  return parts.join(" ");
}

function wordToken(timeMs, endMs, text) {
  return {
    time_ms: timeMs,
    end_ms: endMs,
    text,
    roman: null,
  };
}

export function parseTtml(ttml) {
  const tokens = String(ttml).match(/<\/?[^>]+>|[^<]+/g) ?? [];
  const lines = [];
  let inP = false;
  let inBg = false;
  let inTranslation = false;
  let inRoman = false;
  let rubyTextDepth = 0;
  let pBegin = null;
  let pEnd = null;
  let words = [];
  let wordHasExplicitEnd = [];
  let bgWords = [];
  let textBuf = "";
  let romanBuf = "";
  let currentBegin = null;
  let currentEnd = null;
  const timingStack = [];
  const roleStack = [];
  const rubyStack = [];

  for (const token of tokens) {
    if (token.startsWith("</")) {
      const name = localName(token);
      if (name === "p" && inP) {
        const text = textBuf.trim();
        if (pBegin !== null && text.length > 0) {
          if (pEnd !== null && words.length > 0) {
            const last = words.length - 1;
            if (!wordHasExplicitEnd[last]) {
              words[last].end_ms = pEnd;
            }
          }
          lines.push({
            time_ms: pBegin,
            text,
            words: words.length > 0 ? words : null,
            bg_words: bgWords.length > 0 ? bgWords : null,
            section: null,
            roman: nonemptyTrimmed(romanBuf) ?? joinedWordRomans(words),
          });
        }
        inP = false;
        continue;
      }
      if (name === "span") {
        const role = roleStack.pop();
        if (role === "x-translation") inTranslation = false;
        if (role === "x-roman") inRoman = false;
        if (role === "x-bg") inBg = false;
        const previous = timingStack.pop();
        if (previous) {
          currentBegin = previous.begin;
          currentEnd = previous.end;
        } else {
          currentBegin = null;
          currentEnd = null;
        }
        if (rubyStack.pop()) {
          rubyTextDepth = Math.max(0, rubyTextDepth - 1);
        }
      }
      continue;
    }

    if (token.startsWith("<")) {
      if (token.startsWith("<!")) continue;
      const selfClosing = /\/>$/.test(token);
      const name = localName(token);
      const attrs = parseAttrs(token);
      if (name === "p") {
        if (selfClosing) continue;
        inP = true;
        pBegin = parseTtmlTimestamp(attrs.begin);
        pEnd = parseTtmlTimestamp(attrs.end);
        words = [];
        wordHasExplicitEnd = [];
        bgWords = [];
        textBuf = "";
        romanBuf = "";
        currentBegin = null;
        currentEnd = null;
        continue;
      }
      if (name === "span") {
        if (selfClosing) continue;
        const role = attrs.role ?? "";
        const isRubyText =
          attrs.ruby === "text" || attrs.ruby === "textContainer";
        roleStack.push(role);
        timingStack.push({ begin: currentBegin, end: currentEnd });
        rubyStack.push(isRubyText);
        if (isRubyText) rubyTextDepth += 1;
        if (role === "x-translation") inTranslation = true;
        else if (role === "x-roman") inRoman = true;
        else if (role === "x-bg") inBg = true;
        else if (inP && attrs.begin) {
          currentBegin = parseTtmlTimestamp(attrs.begin);
          currentEnd = parseTtmlTimestamp(attrs.end);
        }
      }
      continue;
    }

    if (!inP) continue;
    if (inTranslation || rubyTextDepth > 0) continue;
    if (isPrettyPrintSpace(token)) continue;

    if (inRoman) {
      if (inBg) {
        appendWordRoman(bgWords[bgWords.length - 1], token);
        continue;
      }
      if (currentBegin !== null) {
        appendWordRoman(words[words.length - 1], token);
      } else {
        romanBuf += token;
      }
      continue;
    }

    if (inBg) {
      if (currentBegin !== null) {
        bgWords.push(
          wordToken(
            currentBegin,
            currentEnd ?? currentBegin + 500,
            token.trim(),
          ),
        );
      }
      continue;
    }

    textBuf += token;
    if (currentBegin !== null) {
      words.push(
        wordToken(currentBegin, currentEnd ?? currentBegin + 500, token.trim()),
      );
      wordHasExplicitEnd.push(currentEnd !== null);
    }
  }

  lines.sort((a, b) => a.time_ms - b.time_ms);
  return lines;
}
