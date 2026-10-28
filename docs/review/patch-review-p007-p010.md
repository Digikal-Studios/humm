# Patch review: P-007 and P-010

Reviewed the commits below on `audit/architecture-review`. Product baseline remains `125a54da3d284ba3be651b4c99a4456ce56cd76a`. Buzz was read from `/tmp/audit-sources/buzz` at `298dbf003505cf945bdfd1eb8e705348b59a5b34` (`whisper_cpp.py` in that tree). That clone is shallow, so `aaf98634` and `eadf4ec352` are not objects here; the v1.4.5 pin was checked in `scripts/ci_prepare.py`, and the word-flush behavior was checked in the file at `298dbf0`.

No product source was edited for this review.

## Verdicts

| ID | SHA | Verdict |
|---|---|---|
| P-007 | `89489dac2ff22c8fc3543cc2ccdafb51faddbe73` | **PASS** |
| P-010 | `bd2c7d720c12dc4ac745189b34cc64fde938b623` | **PASS** |

## P-007 — PASS

`89489dac2ff22c8fc3543cc2ccdafb51faddbe73` changes only `crates/humm-whisper/src/words.rs`. The final flush is the only lossy path, and it matches Buzz.

Buzz `whisper_cpp.py` at this checkout:

- `_append_word` (lines 135–154) returns `False` on `UnicodeDecodeError` and emits nothing.
- The space flush (line 374) and the comma flush (line 380) ignore that return and drop the buffer.
- Only the loop’s last buffer (lines 390–393) does `decode("utf-8", errors="replace").rstrip("\ufffd").strip()`.
- The non-space path (lines 351–352) calls `_flush_complete_chars` on the remainder and has no `rstrip` fallback.

humm follows that split. `append_word` still returns without pushing when `str::from_utf8` fails. The space call (`words.rs` line 81) and the comma call (line 89) ignore the bool. `push_lossy_final_word` runs only when the final `append_word` returns false (lines 104–106). It is `String::from_utf8_lossy`, then `trim_end_matches('\u{FFFD}')`, then `trim`, and it pushes only when that result is non-empty (lines 189–200). `group_characters` and `flush_complete_chars` are unchanged.

The fallback is reachable. `emit_word_segments` (`engine.rs` lines 875–887) is the only production caller of `group_words`. `WhisperToken::to_bytes` (`whisper-rs` 0.16 `token.rs`) returns the raw `CStr` bytes and does not require UTF-8, so a split character can arrive as an incomplete buffer. A strict success still keeps a real trailing U+FFFD; the lossy strip runs only after `from_utf8` fails, which is what `_append_word` does.

A direct transcription of the Buzz loop and the Rust expression agree on the cases that distinguish a wrong strip: trailing `hello\xff` → `hello`; leading `\xffhello` → `\u{FFFD}hello`; a middle `\xff` stays; both ends keep the leading replacement and drop the trailing one; `hello \xff` → `hello`; `hello\xff ` keeps the replacement; a comma flush of `hello\xff` before `, world` drops the bad word; `Bieži vien` plus a trailing `0xC4` → `Bieži vien`. `trim` before `trim_end_matches` on `hello\xff ` yields `hello`, which is the Buzz mismatch. The landed function does not do that.

`cargo +stable test -p humm-whisper --lib words::` on this tree: 8 passed, 0 failed, including `final_flush_strips_only_a_trailing_replacement_character`. Those three asserts fail a global U+FFFD strip, a both-ends strip, and a lossy decode on the mid-loop space flush. That is the wrong fix the change was written to avoid.

### Nonblocking

The three asserts do not include whitespace after the invalid byte (`hello\xff `). An implementation that trims before stripping U+FFFD still passes them and disagrees with Buzz on that buffer. The function in this SHA already uses the Buzz order.

Segment text is a separate path. With `word_timestamps` false (`domain.rs` default), `Bridge::on_segment` (`engine.rs` line 537) only `trim`s the lossy callback string. Live text uses `segment.to_str_lossy` (`engine.rs` lines 360–362) and does not call `group_words`. Buzz `_parse_segment_timings` (lines 406–409) and `test_parse_segment_timings_handles_truncated_utf8_without_mojibake` strip a trailing U+FFFD from segment text (`Bieži vien` plus `0xC4`). That path keeps the segment; it does not drop a word. It is outside this commit.

### Uncertain

None that would change the word-flush result.

## P-010 — PASS

`bd2c7d720c12dc4ac745189b34cc64fde938b623` inserts one sentence after the existing ground-truth line in `docs/analysis/FEATURE_INVENTORY.md` and `docs/analysis/EXTERNAL_SURFACE.md`. The diff against `125a54da` for those two files is those four lines. No feature row or section body changed.

The headers stay on v1.4.5. Inventory line 3 is still “Ground truth: … (v1.4.5)”. External surface line 3 is still “Source of truth: … v1.4.5”. F-020 still lists the old extension set and still cites `transcriber.py:222-225`.

The new sentence names `298dbf003505cf945bdfd1eb8e705348b59a5b34` as the behavior target, says each document was written against `aaf98634cd354be207e00cc5aaa1bfbd9e19cf22`, and says it has not been re-read against that main commit. It says the rows and sections below still describe the v1.4.5 tree. That wording does not claim the inventory was checked against `298dbf0`. The full hashes match the ledger and the buzz checkout HEAD.

`scripts/ci_prepare.py` is not in the commit. `BUZZ_COMMIT` is still `aaf98634cd354be207e00cc5aaa1bfbd9e19cf22`, and the two fixture blob hashes in that file are unchanged. `docs/ci.md` still points at the same commit.

### Nonblocking

None. “Known gaps are in `docs/review/ledger.md`” matches where the confirmed post-tag gaps are listed. The sentence does not say that list was re-derived from a full read of main.

### Uncertain

None. The shallow buzz clone does not contain the `aaf98634` object, so this review did not re-resolve the tag. The pin text matches the hash already used as official v1.4.5 in `scripts/ci_prepare.py`.
