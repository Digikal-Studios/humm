#!/usr/bin/env python3
"""ADR-002 quality proxy: pairwise transcript agreement (WER-proxy, no ground truth).

Normalizes two transcripts (lowercase, strip punctuation, collapse whitespace)
and computes a word-level Levenshtein edit distance. Agreement % =
1 - edit_distance / max(word_count_a, word_count_b).
"""
import re
import sys


def read_humm(path):
    """humm writes a plain .txt transcript."""
    with open(path, encoding="utf-8") as f:
        return f.read()


def read_candle(path):
    """candle's example prints window headers ('0.0s -- 30.0s'), empty
    '0.0s-0.0s:' lines, and per-segment lines '  A.As-B.Bs:  <text>'.
    Extract only the segment text."""
    seg = re.compile(r"^\s*\d+\.\d+s-\d+\.\d+s:\s*(.*)$")
    out = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            m = seg.match(line)
            if m:
                out.append(m.group(1))
    return " ".join(out)


def normalize(text):
    text = text.lower()
    # strip punctuation (keep unicode letters/digits/whitespace)
    text = "".join(
        ch if (ch.isalnum() or ch.isspace()) else " "
        for ch in text
    )
    # collapse whitespace
    return re.sub(r"\s+", " ", text).strip()


def word_edit_distance(a, b):
    """Levenshtein distance over word lists (iterative DP, O(n*m))."""
    if len(a) < len(b):
        a, b = b, a
    prev = list(range(len(b) + 1))
    for i, wa in enumerate(a, 1):
        cur = [i]
        for j, wb in enumerate(b, 1):
            cost = 0 if wa == wb else 1
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + cost))
        prev = cur
    return prev[-1]


def sample_sentences(raw, n=3):
    sents = re.split(r"(?<=[.!?])\s+", raw.strip())
    sents = [s.strip() for s in sents if len(s.strip()) > 20]
    return sents[:n]


def main():
    label, humm_path, candle_path = sys.argv[1], sys.argv[2], sys.argv[3]
    humm_raw = read_humm(humm_path)
    candle_raw = read_candle(candle_path)

    a = normalize(humm_raw).split()
    b = normalize(candle_raw).split()
    dist = word_edit_distance(a, b)
    agreement = 1 - dist / max(len(a), len(b))

    print(f"=== {label} ===")
    print(f"whisper-rs words: {len(a)}   candle words: {len(b)}")
    print(f"word edit distance: {dist}")
    print(f"agreement: {agreement * 100:.1f}%")
    print()
    print("--- whisper-rs sample sentences ---")
    for s in sample_sentences(humm_raw):
        print(f"  * {s}")
    print("--- candle sample sentences ---")
    for s in sample_sentences(candle_raw):
        print(f"  * {s}")
    print()


if __name__ == "__main__":
    main()
