//! In-memory 16 kHz mono 16-bit PCM WAV writer (F-061 closure, design spec
//! §10).
//!
//! Ported from Buzz's `RecordingTranscriber._transcribe_via_api`
//! (`buzz/transcriber/recording_transcriber.py:306-317`), which writes each
//! accepted live chunk to a *temp file* on disk before uploading it:
//!
//! ```python
//! pcm_data = (samples * 32767).astype(np.int16).tobytes()
//! temp_file = tempfile.NamedTemporaryFile(delete=False, suffix=".wav")
//! temp_filename = temp_file.name
//! with wave.open(temp_filename, "wb") as wf:
//!     wf.setnchannels(1)
//!     wf.setsampwidth(2)
//!     wf.setframerate(self.sample_rate)
//!     wf.writeframes(pcm_data)
//! ```
//!
//! **Correction to the design spec's own sketch:** Buzz writes this WAV via
//! the Python stdlib's `wave` module, **not** `soundfile` (the spec's §10
//! prose speculates "soundfile" without re-checking; the import list at
//! `recording_transcriber.py:6` shows `import wave`, and there is no
//! `soundfile` import anywhere in this file). The stdlib `wave` module's
//! `setnchannels(1)`/`setsampwidth(2)`/`setframerate(..)`/`writeframes(..)`
//! produce exactly the canonical 44-byte-header RIFF/WAVE container this
//! module writes: PCM format 1, mono, 16-bit signed little-endian samples,
//! no extension chunks.
//!
//! This port builds the identical byte layout **in memory** rather than on
//! disk (design spec ledger #L13): no temp file, no cleanup path required on
//! any exit path, same wire bytes handed to the multipart upload.
//!
//! **Deviation from Buzz's scaling, documented honestly:** Buzz's
//! `(samples * 32767).astype(np.int16)` never clamps its input to
//! `[-1.0, 1.0]` first — a numpy `astype` cast on an out-of-range float is
//! implementation-defined (platform C-style truncation/wraparound, not a
//! saturating clamp). This port clamps every sample to `[-1.0, 1.0]` before
//! scaling, so out-of-range input (a capture-side bug, or a resampler
//! overshoot) degrades to a clean full-scale sample instead of silently
//! wrapping to an unrelated value. For in-range input the two conversions
//! agree bit-for-bit: both scale by `32767.0` and truncate the fractional
//! part toward zero (numpy's C-style cast and Rust's `as i16` cast both
//! truncate toward zero for in-range floats — Rust's float-to-int `as` cast
//! only *saturates* when the value would otherwise fall outside the target
//! type's range, which the clamp already prevents). Bit-exactness with
//! `wave`/`soundfile`'s own encoder is **not** required here — the server on
//! the receiving end transcribes audio, it does not diff bytes.
//!
//! Note the asymmetry this scaling factor produces: `1.0 * 32767.0 =
//! 32767.0` but `-1.0 * 32767.0 = -32767.0`, **not** `-32768` — the full
//! `i16` range's negative extreme (`-32768`) is simply never emitted. This
//! matches Buzz's own scaling factor exactly (it also multiplies by
//! `32767`, not `32768`), so it is not a humm-only quirk.
//!
//! **Generalized for M8 (`humm-dfn`, F-112):** the writer was originally
//! hardcoded to [`SAMPLE_RATE`] (16 kHz, this module's only caller at the
//! time). [`write_wav_mono_i16`] takes the sample rate as a parameter
//! instead — DeepFilterNet3 writes its denoised output at 48 kHz (queried
//! from the model, `humm-dfn`'s own concern, not hardcoded here).
//! [`write_wav_16k_mono_i16`] is now a thin wrapper calling it with
//! [`SAMPLE_RATE`], so every existing caller/test is unaffected.

use std::io::Write;

use crate::boundary::SAMPLE_RATE;

/// WAV channel count this writer always uses — every caller decodes/
/// resamples to mono before it reaches here, so this is a fixed constant,
/// not a parameter.
const NUM_CHANNELS: u16 = 1;
/// WAV bit depth this writer always uses (16-bit signed PCM).
const BITS_PER_SAMPLE: u16 = 16;
/// Bytes per sample frame: `NUM_CHANNELS * BITS_PER_SAMPLE / 8`.
const BLOCK_ALIGN: u16 = NUM_CHANNELS * (BITS_PER_SAMPLE / 8);

/// Writes `pcm` (mono `f32` samples at `sample_rate` Hz, expected in
/// `[-1.0, 1.0]` — clamped if not, see the [module doc](self)) as a
/// canonical 44-byte-header RIFF/WAVE file to `w`: PCM format 1, 1 channel,
/// `sample_rate` Hz, 16-bit signed little-endian samples, `data` chunk
/// immediately following the header with no extension chunks.
///
/// A thin composition of [`write_wav_header_mono_i16`] (sized for
/// `pcm.len()` samples) followed by [`write_wav_samples_mono_i16`] — kept as
/// its own function since most callers have the whole buffer in hand and
/// don't need the two halves split apart (M9, D5b-6 introduced the split so
/// `humm-dfn` could stream samples to a file as they're produced instead of
/// accumulating a full in-memory buffer first).
///
/// # Errors
///
/// Propagates any [`std::io::Error`] from writing to `w`.
pub fn write_wav_mono_i16(
    pcm: &[f32],
    sample_rate: u32,
    w: &mut impl Write,
) -> std::io::Result<()> {
    write_wav_header_mono_i16(pcm.len(), sample_rate, w)?;
    write_wav_samples_mono_i16(pcm, w)
}

/// Writes just the 44-byte canonical RIFF/WAVE header for `num_samples`
/// mono 16-bit PCM samples at `sample_rate` Hz — no sample data. Pair with
/// [`write_wav_samples_mono_i16`] to stream the data chunk incrementally
/// (e.g. one hop at a time) once the *total* sample count is known up
/// front, without holding the samples themselves in memory all at once.
///
/// `num_samples` must be the exact final count that will be written via
/// [`write_wav_samples_mono_i16`] afterward — this function has no way to
/// patch the header later; a caller that can't compute the total in
/// advance must seek back and rewrite the `RIFF`/`data` size fields itself
/// once the real count is known.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] from writing to `w`.
pub fn write_wav_header_mono_i16(
    num_samples: usize,
    sample_rate: u32,
    w: &mut impl Write,
) -> std::io::Result<()> {
    let byte_rate = sample_rate * u32::from(BLOCK_ALIGN);
    let data_len = num_samples as u32 * u32::from(BLOCK_ALIGN);
    let chunk_size = 36 + data_len;

    w.write_all(b"RIFF")?;
    w.write_all(&chunk_size.to_le_bytes())?;
    w.write_all(b"WAVE")?;

    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // Subchunk1Size = 16 (PCM)
    w.write_all(&1u16.to_le_bytes())?; // AudioFormat = 1 (PCM)
    w.write_all(&NUM_CHANNELS.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&BLOCK_ALIGN.to_le_bytes())?;
    w.write_all(&BITS_PER_SAMPLE.to_le_bytes())?;

    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())?;

    Ok(())
}

/// Writes just the sample-data bytes for `pcm` (mono `f32` samples,
/// expected in `[-1.0, 1.0]` — clamped if not, see the [module doc](self)):
/// each sample clamped then scaled to a 16-bit signed little-endian PCM
/// value, with no header. Call this any number of times in sequence — once
/// per hop/chunk as samples become available — after a single upfront
/// [`write_wav_header_mono_i16`] sized for the eventual total, to stream a
/// WAV file's data chunk without ever holding the whole signal in memory at
/// once.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] from writing to `w`.
pub fn write_wav_samples_mono_i16(pcm: &[f32], w: &mut impl Write) -> std::io::Result<()> {
    for &sample in pcm {
        let clamped = sample.clamp(-1.0, 1.0);
        let scaled = (clamped * 32767.0) as i16;
        w.write_all(&scaled.to_le_bytes())?;
    }

    Ok(())
}

/// Writes `pcm` (16 kHz mono `f32` samples) as a canonical WAV file to `w` —
/// a thin wrapper over [`write_wav_mono_i16`] at [`SAMPLE_RATE`], kept for
/// the live-recording upload path (`crate::openai::live`) that predates the
/// M8 generalization above.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] from writing to `w`.
pub fn write_wav_16k_mono_i16(pcm: &[f32], w: &mut impl Write) -> std::io::Result<()> {
    write_wav_mono_i16(pcm, SAMPLE_RATE, w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_bytes_for_four_known_samples() {
        // Header, byte by byte (every multi-byte field little-endian):
        //   "RIFF"                              52 49 46 46
        //   ChunkSize = 36 + 8 = 44              2C 00 00 00
        //   "WAVE"                               57 41 56 45
        //   "fmt "                               66 6D 74 20
        //   Subchunk1Size = 16                   10 00 00 00
        //   AudioFormat = 1 (PCM)                 01 00
        //   NumChannels = 1                       01 00
        //   SampleRate = 16000 (0x3E80)           80 3E 00 00
        //   ByteRate = 32000 (0x7D00)             00 7D 00 00
        //   BlockAlign = 2                        02 00
        //   BitsPerSample = 16                    10 00
        //   "data"                                64 61 74 61
        //   Subchunk2Size = 4 samples * 2B = 8     08 00 00 00
        //
        // Samples -> i16 (clamp to [-1,1], * 32767.0, truncate toward zero):
        //   0.0  -> 0            (0x0000)
        //   1.0  -> 32767        (0x7FFF)
        //   -1.0 -> -32767       (0x8001 two's complement)
        //   0.5  -> 16383        (0.5*32767=16383.5, truncates to 16383 = 0x3FFF)
        let pcm = [0.0f32, 1.0, -1.0, 0.5];
        let mut out = Vec::new();
        write_wav_16k_mono_i16(&pcm, &mut out).expect("write");

        #[rustfmt::skip]
        let expected: Vec<u8> = vec![
            0x52, 0x49, 0x46, 0x46, // "RIFF"
            0x2C, 0x00, 0x00, 0x00, // ChunkSize = 44
            0x57, 0x41, 0x56, 0x45, // "WAVE"
            0x66, 0x6D, 0x74, 0x20, // "fmt "
            0x10, 0x00, 0x00, 0x00, // Subchunk1Size = 16
            0x01, 0x00,             // AudioFormat = 1
            0x01, 0x00,             // NumChannels = 1
            0x80, 0x3E, 0x00, 0x00, // SampleRate = 16000
            0x00, 0x7D, 0x00, 0x00, // ByteRate = 32000
            0x02, 0x00,             // BlockAlign = 2
            0x10, 0x00,             // BitsPerSample = 16
            0x64, 0x61, 0x74, 0x61, // "data"
            0x08, 0x00, 0x00, 0x00, // Subchunk2Size = 8
            0x00, 0x00,             // 0.0   -> 0
            0xFF, 0x7F,             // 1.0   -> 32767
            0x01, 0x80,             // -1.0  -> -32767
            0xFF, 0x3F,             // 0.5   -> 16383
        ];

        assert_eq!(out.len(), 52);
        assert_eq!(out, expected);
    }

    #[test]
    fn clamp_out_of_range_samples_saturate_to_32767_not_32768() {
        // Pins the asymmetry documented in the module doc comment: the
        // clamp bounds magnitude to 1.0, and 1.0 * 32767.0 = 32767 while
        // -1.0 * 32767.0 = -32767 -- neither direction ever reaches the
        // full i16 range (32768/-32768).
        let pcm = [2.0f32, -2.0, 1.5, -1.5];
        let mut out = Vec::new();
        write_wav_16k_mono_i16(&pcm, &mut out).expect("write");

        let data = &out[44..];
        assert_eq!(i16::from_le_bytes([data[0], data[1]]), 32767);
        assert_eq!(i16::from_le_bytes([data[2], data[3]]), -32767);
        assert_eq!(i16::from_le_bytes([data[4], data[5]]), 32767);
        assert_eq!(i16::from_le_bytes([data[6], data[7]]), -32767);
    }

    #[test]
    fn empty_pcm_writes_header_only_with_zero_length_data_chunk() {
        let mut out = Vec::new();
        write_wav_16k_mono_i16(&[], &mut out).expect("write");

        assert_eq!(out.len(), 44);
        assert_eq!(&out[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([out[4], out[5], out[6], out[7]]), 36);
        assert_eq!(&out[8..12], b"WAVE");
        assert_eq!(&out[36..40], b"data");
        assert_eq!(u32::from_le_bytes([out[40], out[41], out[42], out[43]]), 0);
    }

    // -- write_wav_mono_i16 (M8 generalization, F-112) --------------------

    #[test]
    fn write_wav_mono_i16_at_48k_stamps_the_given_sample_rate_and_byte_rate() {
        // `humm-dfn` writes its DeepFilterNet3 output at 48 kHz — a
        // different rate than the 16 kHz `write_wav_16k_mono_i16` wrapper
        // always uses, so this pins the header fields the generalized
        // function computes from its `sample_rate` parameter.
        let pcm = [0.0f32, 1.0, -1.0, 0.5];
        let mut out = Vec::new();
        write_wav_mono_i16(&pcm, 48_000, &mut out).expect("write");

        assert_eq!(out.len(), 52);
        assert_eq!(&out[0..4], b"RIFF");
        assert_eq!(&out[8..12], b"WAVE");
        assert_eq!(
            u32::from_le_bytes([out[24], out[25], out[26], out[27]]),
            48_000,
            "SampleRate field"
        );
        assert_eq!(
            u32::from_le_bytes([out[28], out[29], out[30], out[31]]),
            48_000 * 2,
            "ByteRate field = sample_rate * block_align(2)"
        );
        assert_eq!(u16::from_le_bytes([out[32], out[33]]), 2, "BlockAlign");
        assert_eq!(u16::from_le_bytes([out[34], out[35]]), 16, "BitsPerSample");

        let data = &out[44..];
        assert_eq!(i16::from_le_bytes([data[0], data[1]]), 0);
        assert_eq!(i16::from_le_bytes([data[2], data[3]]), 32767);
        assert_eq!(i16::from_le_bytes([data[4], data[5]]), -32767);
        assert_eq!(i16::from_le_bytes([data[6], data[7]]), 16383);
    }

    #[test]
    fn write_wav_16k_mono_i16_matches_write_wav_mono_i16_at_sample_rate() {
        let pcm = [0.1f32, -0.2, 0.3, -0.4];
        let mut via_wrapper = Vec::new();
        write_wav_16k_mono_i16(&pcm, &mut via_wrapper).expect("write");
        let mut via_general = Vec::new();
        write_wav_mono_i16(&pcm, SAMPLE_RATE, &mut via_general).expect("write");

        assert_eq!(via_wrapper, via_general);
    }

    // -- write_wav_header_mono_i16 / write_wav_samples_mono_i16 (M9,
    // D5b-6: streaming split for humm-dfn's output rework) ----------------

    #[test]
    fn header_alone_matches_the_first_44_bytes_of_the_combined_writer() {
        let pcm = [0.0f32, 1.0, -1.0, 0.5];
        let mut combined = Vec::new();
        write_wav_mono_i16(&pcm, 48_000, &mut combined).expect("write");

        let mut header_only = Vec::new();
        write_wav_header_mono_i16(pcm.len(), 48_000, &mut header_only).expect("write header");

        assert_eq!(header_only.len(), 44, "header is always exactly 44 bytes");
        assert_eq!(header_only, combined[..44]);
    }

    #[test]
    fn header_data_len_and_chunk_size_scale_with_num_samples_not_a_slice_length() {
        // The header function takes a bare `usize` count rather than a
        // slice — this pins that its size math (RIFF ChunkSize and the
        // `data` Subchunk2Size) is purely a function of that count, exactly
        // matching what `write_wav_mono_i16` would compute for a same-length
        // slice, for several sizes including zero.
        for num_samples in [0usize, 1, 2, 100_000] {
            let mut header_only = Vec::new();
            write_wav_header_mono_i16(num_samples, 16_000, &mut header_only).expect("write header");

            let data_len = num_samples as u32 * u32::from(BLOCK_ALIGN);
            let expected_chunk_size = 36 + data_len;
            assert_eq!(
                u32::from_le_bytes([
                    header_only[4],
                    header_only[5],
                    header_only[6],
                    header_only[7]
                ]),
                expected_chunk_size,
                "RIFF ChunkSize for num_samples={num_samples}"
            );
            assert_eq!(
                u32::from_le_bytes([
                    header_only[40],
                    header_only[41],
                    header_only[42],
                    header_only[43]
                ]),
                data_len,
                "data Subchunk2Size for num_samples={num_samples}"
            );
        }
    }

    #[test]
    fn header_then_samples_written_separately_equals_the_combined_writer() {
        let pcm = [0.0f32, 1.0, -1.0, 0.5, 2.0, -2.0];
        let mut combined = Vec::new();
        write_wav_mono_i16(&pcm, 44_100, &mut combined).expect("write");

        let mut streamed = Vec::new();
        write_wav_header_mono_i16(pcm.len(), 44_100, &mut streamed).expect("write header");
        write_wav_samples_mono_i16(&pcm, &mut streamed).expect("write samples");

        assert_eq!(streamed, combined);
    }

    #[test]
    fn samples_can_be_appended_across_multiple_calls_matching_one_call() {
        // The whole point of splitting the samples writer out: a caller can
        // call it once per hop/chunk as data becomes available, rather than
        // needing the full buffer up front. This pins that two calls with
        // slices back-to-back produce identical bytes to one call with the
        // concatenation.
        let hop_a = [0.1f32, -0.2, 0.3];
        let hop_b = [-0.4f32, 0.5, -0.6, 0.7];

        let mut in_two_calls = Vec::new();
        write_wav_samples_mono_i16(&hop_a, &mut in_two_calls).expect("write hop a");
        write_wav_samples_mono_i16(&hop_b, &mut in_two_calls).expect("write hop b");

        let mut all_at_once = Vec::new();
        let concatenated: Vec<f32> = hop_a.iter().chain(hop_b.iter()).copied().collect();
        write_wav_samples_mono_i16(&concatenated, &mut all_at_once).expect("write concatenated");

        assert_eq!(in_two_calls, all_at_once);
    }
}
