//! Synthetic MP4 builder for tests.
//!
//! Builds minimal valid MP4 byte sequences programmatically so tests
//! are self-contained with no external fixture files.

/// Write a big-endian u16.
fn w16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Write a big-endian u32.
fn w32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Write a big-endian u64.
fn w64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Wrap data in a box with the given fourcc.
fn make_box(fourcc: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = (8 + body.len()) as u32;
    let mut buf = Vec::with_capacity(size as usize);
    w32(&mut buf, size);
    buf.extend_from_slice(fourcc);
    buf.extend_from_slice(body);
    buf
}

/// Build a full-box body (version + flags prefix).
fn full_box_body(version: u8, flags: u32, content: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + content.len());
    buf.push(version);
    buf.push((flags >> 16) as u8);
    buf.push((flags >> 8) as u8);
    buf.push(flags as u8);
    buf.extend_from_slice(content);
    buf
}

/// Build an ftyp box.
fn build_ftyp() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"isom"); // major_brand
    w32(&mut body, 0x200); // minor_version
    body.extend_from_slice(b"isom"); // compatible_brand
    body.extend_from_slice(b"iso2"); // compatible_brand
    make_box(b"ftyp", &body)
}

/// Build an mvhd box (version 0).
fn build_mvhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut content = Vec::new();
    w32(&mut content, 0); // creation_time
    w32(&mut content, 0); // modification_time
    w32(&mut content, timescale);
    w32(&mut content, duration);
    w32(&mut content, 0x00010000); // rate (1.0 fixed-point)
    w16(&mut content, 0x0100); // volume (1.0 fixed-point)
    content.extend_from_slice(&[0u8; 10]); // reserved
                                           // identity matrix (9 * 4 = 36 bytes)
    for &v in &[0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
        w32(&mut content, v);
    }
    content.extend_from_slice(&[0u8; 24]); // pre_defined
    w32(&mut content, 2); // next_track_id

    let body = full_box_body(0, 0, &content);
    make_box(b"mvhd", &body)
}

/// Build a tkhd box (version 0).
fn build_tkhd(track_id: u32, duration: u32, width: u32, height: u32) -> Vec<u8> {
    let mut content = Vec::new();
    w32(&mut content, 0); // creation_time
    w32(&mut content, 0); // modification_time
    w32(&mut content, track_id);
    w32(&mut content, 0); // reserved
    w32(&mut content, duration);
    w64(&mut content, 0); // reserved
    w16(&mut content, 0); // layer
    w16(&mut content, 0); // alternate_group
    w16(&mut content, 0); // volume
    w16(&mut content, 0); // reserved
                          // identity matrix
    for &v in &[0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
        w32(&mut content, v);
    }
    w32(&mut content, width << 16); // width (16.16 fixed-point)
    w32(&mut content, height << 16); // height (16.16 fixed-point)

    let body = full_box_body(0, 3, &content); // flags=3 (enabled + in_movie)
    make_box(b"tkhd", &body)
}

/// Build an mdhd box (version 0).
fn build_mdhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut content = Vec::new();
    w32(&mut content, 0); // creation_time
    w32(&mut content, 0); // modification_time
    w32(&mut content, timescale);
    w32(&mut content, duration);
    w32(&mut content, 0); // language + pre_defined

    let body = full_box_body(0, 0, &content);
    make_box(b"mdhd", &body)
}

/// Build an hdlr box.
fn build_hdlr(handler_type: &[u8; 4]) -> Vec<u8> {
    let mut content = Vec::new();
    w32(&mut content, 0); // pre_defined
    content.extend_from_slice(handler_type);
    content.extend_from_slice(&[0u8; 12]); // reserved
    content.push(0); // name (null-terminated)

    let body = full_box_body(0, 0, &content);
    make_box(b"hdlr", &body)
}

/// Build an stsd box with an avc1 entry.
fn build_stsd_avc1(width: u16, height: u16) -> Vec<u8> {
    // Build a minimal avc1 sample entry
    let mut entry = Vec::new();
    entry.extend_from_slice(&[0u8; 6]); // reserved
    w16(&mut entry, 1); // data_reference_index
    entry.extend_from_slice(&[0u8; 16]); // pre_defined + reserved
    w16(&mut entry, width);
    w16(&mut entry, height);
    w32(&mut entry, 0x00480000); // horiz_resolution (72 dpi)
    w32(&mut entry, 0x00480000); // vert_resolution (72 dpi)
    w32(&mut entry, 0); // reserved
    w16(&mut entry, 1); // frame_count
    entry.extend_from_slice(&[0u8; 32]); // compressor_name
    w16(&mut entry, 0x0018); // depth
    w16(&mut entry, 0xFFFF); // pre_defined (-1)

    // Add a minimal avcC sub-box
    let mut avcc_body = vec![
        1,    // configurationVersion
        0x42, // AVCProfileIndication (Baseline)
        0xC0, // profile_compatibility
        0x1E, // AVCLevelIndication (3.0)
        0xFF, // lengthSizeMinusOne = 3
        0xE1, // numOfSequenceParameterSets = 1
    ];
    w16(&mut avcc_body, 4); // SPS length
    avcc_body.extend_from_slice(&[0x67, 0x42, 0xC0, 0x1E]); // fake SPS
    avcc_body.push(1); // numOfPictureParameterSets = 1
    w16(&mut avcc_body, 4); // PPS length
    avcc_body.extend_from_slice(&[0x68, 0xCE, 0x38, 0x80]); // fake PPS

    let avcc_box = make_box(b"avcC", &avcc_body);
    entry.extend_from_slice(&avcc_box);

    // Wrap in a sized entry
    let entry_size = (8 + entry.len()) as u32;
    let mut sized_entry = Vec::new();
    w32(&mut sized_entry, entry_size);
    sized_entry.extend_from_slice(b"avc1");
    sized_entry.extend_from_slice(&entry);

    // stsd body: full-box header + entry_count + entry
    let mut content = Vec::new();
    w32(&mut content, 1); // entry_count
    content.extend_from_slice(&sized_entry);

    let body = full_box_body(0, 0, &content);
    make_box(b"stsd", &body)
}

/// Build an stsd box with an mp4a entry.
fn build_stsd_mp4a(sample_rate: u32, channel_count: u16) -> Vec<u8> {
    let mut entry = Vec::new();
    entry.extend_from_slice(&[0u8; 6]); // reserved
    w16(&mut entry, 1); // data_reference_index
    entry.extend_from_slice(&[0u8; 8]); // reserved
    w16(&mut entry, channel_count);
    w16(&mut entry, 16); // sample_size
    w16(&mut entry, 0); // pre_defined
    w16(&mut entry, 0); // reserved
    w32(&mut entry, sample_rate << 16); // sample_rate (16.16)

    // Add a minimal esds sub-box (just enough to be parseable)
    let esds_body = full_box_body(0, 0, &[0u8; 4]);
    let esds_box = make_box(b"esds", &esds_body);
    entry.extend_from_slice(&esds_box);

    let entry_size = (8 + entry.len()) as u32;
    let mut sized_entry = Vec::new();
    w32(&mut sized_entry, entry_size);
    sized_entry.extend_from_slice(b"mp4a");
    sized_entry.extend_from_slice(&entry);

    let mut content = Vec::new();
    w32(&mut content, 1);
    content.extend_from_slice(&sized_entry);

    let body = full_box_body(0, 0, &content);
    make_box(b"stsd", &body)
}

/// Build an stts box.
fn build_stts(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut content = Vec::new();
    w32(&mut content, entries.len() as u32);
    for &(count, delta) in entries {
        w32(&mut content, count);
        w32(&mut content, delta);
    }
    let body = full_box_body(0, 0, &content);
    make_box(b"stts", &body)
}

/// Build an stsc box.
fn build_stsc(entries: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut content = Vec::new();
    w32(&mut content, entries.len() as u32);
    for &(first_chunk, samples_per_chunk, desc_index) in entries {
        w32(&mut content, first_chunk);
        w32(&mut content, samples_per_chunk);
        w32(&mut content, desc_index);
    }
    let body = full_box_body(0, 0, &content);
    make_box(b"stsc", &body)
}

/// Build an stsz box with individual sample sizes.
fn build_stsz(sizes: &[u32]) -> Vec<u8> {
    let mut content = Vec::new();
    w32(&mut content, 0); // default_sample_size = 0 (use per-sample)
    w32(&mut content, sizes.len() as u32);
    for &s in sizes {
        w32(&mut content, s);
    }
    let body = full_box_body(0, 0, &content);
    make_box(b"stsz", &body)
}

/// Build an stco box.
fn build_stco(offsets: &[u32]) -> Vec<u8> {
    let mut content = Vec::new();
    w32(&mut content, offsets.len() as u32);
    for &o in offsets {
        w32(&mut content, o);
    }
    let body = full_box_body(0, 0, &content);
    make_box(b"stco", &body)
}

/// Build an stss box.
fn build_stss(sync_samples: &[u32]) -> Vec<u8> {
    let mut content = Vec::new();
    w32(&mut content, sync_samples.len() as u32);
    for &s in sync_samples {
        w32(&mut content, s);
    }
    let body = full_box_body(0, 0, &content);
    make_box(b"stss", &body)
}

/// Configuration for a test track.
pub struct TestTrack {
    pub track_id: u32,
    pub handler: [u8; 4],
    pub timescale: u32,
    pub sample_sizes: Vec<u32>,
    pub sample_delta: u32,
    pub sync_samples: Option<Vec<u32>>,
    pub width: u16,
    pub height: u16,
    pub sample_rate: u32,
    pub channel_count: u16,
}

impl TestTrack {
    pub fn video(width: u16, height: u16, num_samples: u32) -> Self {
        Self {
            track_id: 1,
            handler: *b"vide",
            timescale: 30000,
            sample_sizes: vec![1000; num_samples as usize],
            sample_delta: 1001,          // 29.97fps
            sync_samples: Some(vec![1]), // only first sample is keyframe
            width,
            height,
            sample_rate: 0,
            channel_count: 0,
        }
    }

    pub fn audio(sample_rate: u32, num_samples: u32) -> Self {
        Self {
            track_id: 2,
            handler: *b"soun",
            timescale: sample_rate,
            sample_sizes: vec![512; num_samples as usize],
            sample_delta: 1024,
            sync_samples: None, // all audio samples are sync
            width: 0,
            height: 0,
            sample_rate,
            channel_count: 2,
        }
    }
}

/// Build a complete minimal MP4 byte sequence.
pub fn build_test_mp4(tracks: &[TestTrack]) -> Vec<u8> {
    let ftyp = build_ftyp();

    // Build mdat first so we know the offset
    let mut sample_data = Vec::new();
    for track in tracks {
        for &size in &track.sample_sizes {
            sample_data.extend(vec![0xABu8; size as usize]);
        }
    }
    let mdat = make_box(b"mdat", &sample_data);

    // mdat starts after ftyp + moov, but we need to know moov size first.
    // Build moov, compute offsets, then rebuild with correct offsets.

    // First pass: compute total duration per track and build moov without correct offsets
    let movie_timescale = 1000u32;
    let max_duration = tracks
        .iter()
        .map(|t| {
            let sample_count: u32 = t.sample_sizes.len() as u32;
            (sample_count as u64 * t.sample_delta as u64 * movie_timescale as u64)
                / t.timescale as u64
        })
        .max()
        .unwrap_or(0) as u32;

    // Build moov content
    let mvhd = build_mvhd(movie_timescale, max_duration);

    let mut trak_boxes = Vec::new();
    // We'll need to fixup stco offsets after we know the total moov size
    let mut stco_fixup_info: Vec<(usize, Vec<u32>)> = Vec::new(); // (offset_in_moov, placeholder_offsets)

    let mut mdat_offset_counter = 0u32; // relative offset within mdat body

    for track in tracks {
        let sample_count = track.sample_sizes.len() as u32;
        let track_duration_media = sample_count as u64 * track.sample_delta as u64;
        let track_duration_movie =
            (track_duration_media * movie_timescale as u64) / track.timescale as u64;

        let tkhd = build_tkhd(
            track.track_id,
            track_duration_movie as u32,
            track.width as u32,
            track.height as u32,
        );
        let mdhd = build_mdhd(track.timescale, track_duration_media as u32);
        let hdlr = build_hdlr(&track.handler);

        let stsd = if &track.handler == b"vide" {
            build_stsd_avc1(track.width, track.height)
        } else {
            build_stsd_mp4a(track.sample_rate, track.channel_count)
        };

        let stts = build_stts(&[(sample_count, track.sample_delta)]);
        let stsc = build_stsc(&[(1, 1, 1)]); // 1 sample per chunk

        let stsz = build_stsz(&track.sample_sizes);

        // Build chunk offsets (one chunk per sample) — placeholder offsets for now
        let mut chunk_offsets = Vec::new();
        for &size in &track.sample_sizes {
            chunk_offsets.push(mdat_offset_counter);
            mdat_offset_counter += size;
        }

        let stco = build_stco(&chunk_offsets);

        let mut stbl_body = Vec::new();
        stbl_body.extend_from_slice(&stsd);
        stbl_body.extend_from_slice(&stts);
        stbl_body.extend_from_slice(&stsc);
        stbl_body.extend_from_slice(&stsz);

        // Remember where stco will be so we can fix up offsets
        stco_fixup_info.push((0, chunk_offsets)); // we'll set the real offset later
        stbl_body.extend_from_slice(&stco);

        if let Some(ref sync) = track.sync_samples {
            let stss = build_stss(sync);
            stbl_body.extend_from_slice(&stss);
        }

        let stbl = make_box(b"stbl", &stbl_body);

        // minf needs a media-specific header box (vmhd/smhd) but many parsers tolerate its absence.
        // Add a minimal vmhd or smhd.
        let media_header = if &track.handler == b"vide" {
            let body = full_box_body(0, 1, &[0u8; 8]); // vmhd with flag=1
            make_box(b"vmhd", &body)
        } else {
            let body = full_box_body(0, 0, &[0u8; 4]); // smhd
            make_box(b"smhd", &body)
        };

        // dinf + dref (required by spec, minimal)
        let dref_body = full_box_body(0, 0, &{
            let mut d = Vec::new();
            w32(&mut d, 1); // entry_count
            let url_body = full_box_body(0, 1, &[]); // flag=1 means self-contained
            d.extend_from_slice(&make_box(b"url ", &url_body));
            d
        });
        let dinf = make_box(b"dinf", &make_box(b"dref", &dref_body));

        let mut minf_body = Vec::new();
        minf_body.extend_from_slice(&media_header);
        minf_body.extend_from_slice(&dinf);
        minf_body.extend_from_slice(&stbl);
        let minf = make_box(b"minf", &minf_body);

        let mut mdia_body = Vec::new();
        mdia_body.extend_from_slice(&mdhd);
        mdia_body.extend_from_slice(&hdlr);
        mdia_body.extend_from_slice(&minf);
        let mdia = make_box(b"mdia", &mdia_body);

        let mut trak_body = Vec::new();
        trak_body.extend_from_slice(&tkhd);
        trak_body.extend_from_slice(&mdia);
        let trak = make_box(b"trak", &trak_body);
        trak_boxes.push(trak);
    }

    let mut moov_body = Vec::new();
    moov_body.extend_from_slice(&mvhd);
    for trak in &trak_boxes {
        moov_body.extend_from_slice(trak);
    }
    let moov = make_box(b"moov", &moov_body);

    // Now we know the real mdat body offset: ftyp.len() + moov.len() + 8 (mdat header)
    let mdat_body_offset = (ftyp.len() + moov.len() + 8) as u32;

    // Rebuild moov with corrected stco offsets.
    // Rather than surgically patching bytes, the simplest approach is to
    // rebuild with the correct offsets.
    let mut mdat_sample_offset = mdat_body_offset;
    let corrected_tracks: Vec<TestTrack> = tracks
        .iter()
        .map(|t| {
            // Just clone the track info — we'll rebuild everything
            TestTrack {
                track_id: t.track_id,
                handler: t.handler,
                timescale: t.timescale,
                sample_sizes: t.sample_sizes.clone(),
                sample_delta: t.sample_delta,
                sync_samples: t.sync_samples.clone(),
                width: t.width,
                height: t.height,
                sample_rate: t.sample_rate,
                channel_count: t.channel_count,
            }
        })
        .collect();

    // Rebuild with correct offsets
    let mut trak_boxes2 = Vec::new();
    for track in &corrected_tracks {
        let sample_count = track.sample_sizes.len() as u32;
        let track_duration_media = sample_count as u64 * track.sample_delta as u64;
        let track_duration_movie =
            (track_duration_media * movie_timescale as u64) / track.timescale as u64;

        let tkhd = build_tkhd(
            track.track_id,
            track_duration_movie as u32,
            track.width as u32,
            track.height as u32,
        );
        let mdhd = build_mdhd(track.timescale, track_duration_media as u32);
        let hdlr = build_hdlr(&track.handler);

        let stsd = if &track.handler == b"vide" {
            build_stsd_avc1(track.width, track.height)
        } else {
            build_stsd_mp4a(track.sample_rate, track.channel_count)
        };
        let stts = build_stts(&[(sample_count, track.sample_delta)]);
        let stsc = build_stsc(&[(1, 1, 1)]);
        let stsz = build_stsz(&track.sample_sizes);

        let mut correct_offsets = Vec::new();
        for &size in &track.sample_sizes {
            correct_offsets.push(mdat_sample_offset);
            mdat_sample_offset += size;
        }
        let stco = build_stco(&correct_offsets);

        let mut stbl_body = Vec::new();
        stbl_body.extend_from_slice(&stsd);
        stbl_body.extend_from_slice(&stts);
        stbl_body.extend_from_slice(&stsc);
        stbl_body.extend_from_slice(&stsz);
        stbl_body.extend_from_slice(&stco);
        if let Some(ref sync) = track.sync_samples {
            stbl_body.extend_from_slice(&build_stss(sync));
        }
        let stbl = make_box(b"stbl", &stbl_body);

        let media_header = if &track.handler == b"vide" {
            make_box(b"vmhd", &full_box_body(0, 1, &[0u8; 8]))
        } else {
            make_box(b"smhd", &full_box_body(0, 0, &[0u8; 4]))
        };
        let dref_body = full_box_body(0, 0, &{
            let mut d = Vec::new();
            w32(&mut d, 1);
            d.extend_from_slice(&make_box(b"url ", &full_box_body(0, 1, &[])));
            d
        });
        let dinf = make_box(b"dinf", &make_box(b"dref", &dref_body));

        let minf = make_box(b"minf", &[&media_header[..], &dinf, &stbl].concat());
        let mdia = make_box(b"mdia", &[&mdhd[..], &hdlr, &minf].concat());
        let trak = make_box(b"trak", &[&tkhd[..], &mdia].concat());
        trak_boxes2.push(trak);
    }

    let mut moov_body2 = Vec::new();
    moov_body2.extend_from_slice(&mvhd);
    for trak in &trak_boxes2 {
        moov_body2.extend_from_slice(trak);
    }
    let moov2 = make_box(b"moov", &moov_body2);

    // Verify moov size didn't change (it shouldn't since offsets are same byte count)
    assert_eq!(
        moov.len(),
        moov2.len(),
        "moov size changed during offset fixup"
    );

    let mut result = Vec::new();
    result.extend_from_slice(&ftyp);
    result.extend_from_slice(&moov2);
    result.extend_from_slice(&mdat);
    result
}
