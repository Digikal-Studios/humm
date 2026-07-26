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

use std::io::Write;

use crate::boundary::SAMPLE_RATE;

/// WAV channel count this writer always uses — humm's live-recording
/// pipeline already decodes/resamples to mono before it reaches here (spec
/// §10), so this is a fixed constant, not a parameter.
const NUM_CHANNELS: u16 = 1;
/// WAV bit depth this writer always uses (16-bit signed PCM).
const BITS_PER_SAMPLE: u16 = 16;
/// Bytes per sample frame: `NUM_CHANNELS * BITS_PER_SAMPLE / 8`.
const BLOCK_ALIGN: u16 = NUM_CHANNELS * (BITS_PER_SAMPLE / 8);
/// Bytes per second of audio: `SAMPLE_RATE * BLOCK_ALIGN`.
const BYTE_RATE: u32 = SAMPLE_RATE * (BLOCK_ALIGN as u32);

/// Writes `pcm` (16 kHz mono `f32` samples, expected in `[-1.0, 1.0]` —
/// clamped if not, see the [module doc](self)) as a canonical 44-byte-header
/// RIFF/WAVE file to `w`: PCM format 1, 1 channel, 16000 Hz, 16-bit signed
/// little-endian samples, `data` chunk immediately following the header with
/// no extension chunks.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] from writing to `w`.
pub fn write_wav_16k_mono_i16(pcm: &[f32], w: &mut impl Write) -> std::io::Result<()> {
    let data_len = pcm.len() as u32 * u32::from(BLOCK_ALIGN);
    let chunk_size = 36 + data_len;

    w.write_all(b"RIFF")?;
    w.write_all(&chunk_size.to_le_bytes())?;
    w.write_all(b"WAVE")?;

    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // Subchunk1Size = 16 (PCM)
    w.write_all(&1u16.to_le_bytes())?; // AudioFormat = 1 (PCM)
    w.write_all(&NUM_CHANNELS.to_le_bytes())?;
    w.write_all(&SAMPLE_RATE.to_le_bytes())?;
    w.write_all(&BYTE_RATE.to_le_bytes())?;
    w.write_all(&BLOCK_ALIGN.to_le_bytes())?;
    w.write_all(&BITS_PER_SAMPLE.to_le_bytes())?;

    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())?;

    for &sample in pcm {
        let clamped = sample.clamp(-1.0, 1.0);
        let scaled = (clamped * 32767.0) as i16;
        w.write_all(&scaled.to_le_bytes())?;
    }

    Ok(())
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
}
