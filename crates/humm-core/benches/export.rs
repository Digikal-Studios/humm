//! Export writer benchmarks (Task 9: criterion suite + M1 baselines).
//!
//! Builds 10 000 synthetic segments once and benches [`write_srt`],
//! [`write_vtt`], and [`write_txt`] writing into a reused `Vec<u8>`. No
//! external prerequisites (ffmpeg/model/testdata) — this bench always runs.

use criterion::{Criterion, criterion_group, criterion_main};

use humm_core::domain::{Millis, Segment};
use humm_core::export::{DEFAULT_PARAGRAPH_SPLIT, TextKey, write_srt, write_txt, write_vtt};

const SEGMENT_COUNT: usize = 10_000;

/// 10 000 segments with varying-length text and periodic large gaps, so
/// `write_txt`'s paragraph-split branch is exercised alongside the common
/// path.
fn synthetic_segments(count: usize) -> Vec<Segment> {
    let mut segments = Vec::with_capacity(count);
    let mut t = 0i64;
    for i in 0..count {
        let duration = 500 + (i % 1500) as i64;
        let start = Millis(t);
        let end = Millis(t + duration);
        let text = format!(
            "segment {i} contains a handful of representative words to size the writers realistically"
        );
        segments.push(Segment {
            start,
            end,
            text,
            translation: String::new(),
        });
        // Every 50th segment gets a paragraph-sized gap (>= DEFAULT_PARAGRAPH_SPLIT);
        // the rest get a small, non-paragraph gap.
        let gap = if i % 50 == 0 { 2500 } else { 120 };
        t += duration + gap;
    }
    segments
}

fn bench_export(c: &mut Criterion) {
    let segments = synthetic_segments(SEGMENT_COUNT);
    let mut buf: Vec<u8> = Vec::new();

    let mut group = c.benchmark_group("export");

    group.bench_function("write_srt/10000_segments", |b| {
        b.iter(|| {
            buf.clear();
            write_srt(std::hint::black_box(&segments), TextKey::Text, &mut buf)
                .expect("write_srt should succeed");
            std::hint::black_box(&buf);
        });
    });

    group.bench_function("write_vtt/10000_segments", |b| {
        b.iter(|| {
            buf.clear();
            write_vtt(std::hint::black_box(&segments), TextKey::Text, &mut buf)
                .expect("write_vtt should succeed");
            std::hint::black_box(&buf);
        });
    });

    group.bench_function("write_txt/10000_segments", |b| {
        b.iter(|| {
            buf.clear();
            write_txt(
                std::hint::black_box(&segments),
                TextKey::Text,
                DEFAULT_PARAGRAPH_SPLIT,
                &mut buf,
            )
            .expect("write_txt should succeed");
            std::hint::black_box(&buf);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_export);
criterion_main!(benches);
