//! Resolves MP4 sample table boxes into a flat, seekable index.
//!
//! Merges stts, ctts, stsc, stsz, stco/co64, and stss into a linear
//! list of `SampleEntry` values with precomputed file offsets and timestamps.

use crate::boxes::ctts::CompositionOffsetBox;
use crate::boxes::stco::ChunkOffsetBox;
use crate::boxes::stsc::SampleToChunkBox;
use crate::boxes::stss::SyncSampleBox;
use crate::boxes::stsz::SampleSizeBox;
use crate::boxes::stts::TimeToSampleBox;
use crate::error::Mp4Error;

/// A single resolved sample with all metadata needed for packet construction.
#[derive(Debug, Clone)]
pub struct SampleEntry {
    /// 0-based sample index.
    pub sample_index: u32,
    /// Byte offset of this sample's data in the file.
    pub offset: u64,
    /// Size of this sample in bytes.
    pub size: u32,
    /// Decode timestamp in media timescale ticks.
    pub dts: i64,
    /// Composition time offset (PTS = DTS + cts_offset).
    pub cts_offset: i32,
    /// Whether this sample is a sync (key) frame.
    pub is_keyframe: bool,
}

/// Resolved sample table for a single track.
#[derive(Debug)]
pub struct SampleTable {
    pub entries: Vec<SampleEntry>,
    pub timescale: u32,
}

/// Build a flat sample table from the parsed box data.
pub fn build_sample_table(
    stts: &TimeToSampleBox,
    ctts: Option<&CompositionOffsetBox>,
    stsc: &SampleToChunkBox,
    stsz: &SampleSizeBox,
    chunk_offsets: &ChunkOffsetBox,
    stss: Option<&SyncSampleBox>,
    timescale: u32,
) -> Result<SampleTable, Mp4Error> {
    let sample_count = stsz.sample_count as usize;

    // Build keyframe set (1-indexed). If stss is absent, all samples are sync.
    let all_sync = stss.is_none();
    let sync_set: std::collections::HashSet<u32> = stss
        .map(|s| s.sync_samples.iter().copied().collect())
        .unwrap_or_default();

    // Resolve DTS from stts (run-length encoded deltas)
    let mut dts_values = Vec::with_capacity(sample_count);
    let mut dts: i64 = 0;
    for entry in &stts.entries {
        for _ in 0..entry.sample_count {
            dts_values.push(dts);
            dts += entry.sample_delta as i64;
        }
    }

    // Resolve CTS offsets from ctts
    let mut cts_offsets = Vec::with_capacity(sample_count);
    if let Some(ctts) = ctts {
        for entry in &ctts.entries {
            for _ in 0..entry.sample_count {
                cts_offsets.push(entry.sample_offset);
            }
        }
    }
    // Pad with zeros if ctts has fewer entries or is absent
    cts_offsets.resize(sample_count, 0);

    // Resolve sample sizes
    let sizes: Vec<u32> = if stsz.default_sample_size > 0 {
        vec![stsz.default_sample_size; sample_count]
    } else {
        stsz.sample_sizes.clone()
    };

    if sizes.len() < sample_count {
        return Err(Mp4Error::InvalidBox {
            offset: 0,
            message: format!(
                "stsz has {} sizes but expected {}",
                sizes.len(),
                sample_count
            ),
        });
    }

    // Resolve sample-to-chunk mapping and compute file offsets.
    // stsc entries define runs: from first_chunk until the next entry's first_chunk,
    // each chunk has samples_per_chunk samples.
    let num_chunks = chunk_offsets.offsets.len();
    let mut sample_offsets = Vec::with_capacity(sample_count);
    let mut sample_idx: usize = 0;

    for chunk_idx in 0..num_chunks {
        let chunk_num = chunk_idx as u32 + 1; // 1-indexed

        // Find which stsc entry applies to this chunk
        let stsc_entry = stsc
            .entries
            .iter()
            .rev()
            .find(|e| e.first_chunk <= chunk_num)
            .ok_or_else(|| Mp4Error::InvalidBox {
                offset: 0,
                message: "stsc has no entry for chunk".to_string(),
            })?;

        let mut chunk_offset = chunk_offsets.offsets[chunk_idx];

        for _ in 0..stsc_entry.samples_per_chunk {
            if sample_idx >= sample_count {
                break;
            }
            sample_offsets.push(chunk_offset);
            chunk_offset += sizes[sample_idx] as u64;
            sample_idx += 1;
        }
    }

    // Build final entries
    let resolved_count = sample_offsets.len().min(sample_count);
    let mut entries = Vec::with_capacity(resolved_count);

    for i in 0..resolved_count {
        let sample_num = i as u32 + 1; // 1-indexed for stss comparison
        let is_keyframe = all_sync || sync_set.contains(&sample_num);

        entries.push(SampleEntry {
            sample_index: i as u32,
            offset: sample_offsets[i],
            size: sizes[i],
            dts: dts_values.get(i).copied().unwrap_or(0),
            cts_offset: cts_offsets[i],
            is_keyframe,
        });
    }

    Ok(SampleTable { entries, timescale })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boxes::stsc::StscEntry;
    use crate::boxes::stts::SttsEntry;

    #[test]
    fn test_that_sample_table_resolves_simple_case() {
        // GIVEN — 4 samples, 1 sample per chunk, all keyframes (no stss)
        let stts = TimeToSampleBox {
            entries: vec![SttsEntry {
                sample_count: 4,
                sample_delta: 1000,
            }],
        };
        let stsc = SampleToChunkBox {
            entries: vec![StscEntry {
                first_chunk: 1,
                samples_per_chunk: 1,
                sample_description_index: 1,
            }],
        };
        let stsz = SampleSizeBox {
            default_sample_size: 100,
            sample_sizes: vec![],
            sample_count: 4,
        };
        let stco = ChunkOffsetBox {
            offsets: vec![1000, 1100, 1200, 1300],
        };

        // WHEN
        let table = build_sample_table(&stts, None, &stsc, &stsz, &stco, None, 44100).unwrap();

        // THEN
        assert_eq!(table.entries.len(), 4);
        assert_eq!(table.entries[0].offset, 1000);
        assert_eq!(table.entries[0].dts, 0);
        assert!(table.entries[0].is_keyframe); // all sync when stss absent
        assert_eq!(table.entries[1].dts, 1000);
        assert_eq!(table.entries[2].dts, 2000);
        assert_eq!(table.entries[3].dts, 3000);
    }

    #[test]
    fn test_that_stsc_with_multiple_samples_per_chunk_resolves() {
        // GIVEN — 2 chunks, 2 samples per chunk
        let stts = TimeToSampleBox {
            entries: vec![SttsEntry {
                sample_count: 4,
                sample_delta: 512,
            }],
        };
        let stsc = SampleToChunkBox {
            entries: vec![StscEntry {
                first_chunk: 1,
                samples_per_chunk: 2,
                sample_description_index: 1,
            }],
        };
        let stsz = SampleSizeBox {
            default_sample_size: 0,
            sample_sizes: vec![50, 60, 70, 80],
            sample_count: 4,
        };
        let stco = ChunkOffsetBox {
            offsets: vec![500, 700],
        };

        // WHEN
        let table = build_sample_table(&stts, None, &stsc, &stsz, &stco, None, 48000).unwrap();

        // THEN — samples within a chunk are contiguous
        assert_eq!(table.entries.len(), 4);
        assert_eq!(table.entries[0].offset, 500);
        assert_eq!(table.entries[1].offset, 550); // 500 + 50
        assert_eq!(table.entries[2].offset, 700);
        assert_eq!(table.entries[3].offset, 770); // 700 + 70
    }

    #[test]
    fn test_that_stss_marks_keyframes_correctly() {
        // GIVEN — 4 samples, only sample 1 and 3 are keyframes
        let stts = TimeToSampleBox {
            entries: vec![SttsEntry {
                sample_count: 4,
                sample_delta: 1000,
            }],
        };
        let stsc = SampleToChunkBox {
            entries: vec![StscEntry {
                first_chunk: 1,
                samples_per_chunk: 1,
                sample_description_index: 1,
            }],
        };
        let stsz = SampleSizeBox {
            default_sample_size: 100,
            sample_sizes: vec![],
            sample_count: 4,
        };
        let stco = ChunkOffsetBox {
            offsets: vec![0, 100, 200, 300],
        };
        let stss = SyncSampleBox {
            sync_samples: vec![1, 3], // 1-indexed
        };

        // WHEN
        let table = build_sample_table(&stts, None, &stsc, &stsz, &stco, Some(&stss), 30).unwrap();

        // THEN
        assert!(table.entries[0].is_keyframe);
        assert!(!table.entries[1].is_keyframe);
        assert!(table.entries[2].is_keyframe);
        assert!(!table.entries[3].is_keyframe);
    }
}
