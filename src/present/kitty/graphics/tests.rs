use super::*;
use std::io::Read;

const ID: ImageId = ImageId(0x123456);

fn cells(columns: u16, rows: u16) -> Cells {
    Cells { columns, rows }
}

/// Pixels that do not compress well, to get more than one chunk.
fn noise(len: usize) -> Vec<u8> {
    let mut state: u32 = 1;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (state >> 16) as u8
        })
        .collect()
}

/// The keys and base64 data of each graphics sequence in `out`.
fn sequences(out: &[u8]) -> Vec<(String, String)> {
    let out = std::str::from_utf8(out).unwrap();
    let rest = out.strip_prefix("\x1b_G").expect("starts with a sequence");
    rest.split("\x1b_G")
        .map(|sequence| {
            let body = sequence.strip_suffix("\x1b\\").expect("terminated");
            let (keys, data) = body.split_once(';').expect("keys and data");
            (keys.to_string(), data.to_string())
        })
        .collect()
}

#[test]
fn sends_a_small_image_in_one_sequence() {
    let rgba = [255, 0, 0, 255, 0, 0, 255, 128];
    let pixels = Pixels {
        width: 2,
        height: 1,
        rgba: &rgba,
    };
    let mut out = Vec::new();
    transmit(&mut out, ID, &pixels, cells(1, 1), Effort::Thorough).unwrap();
    let sequences = sequences(&out);
    assert_eq!(sequences.len(), 1);
    assert_eq!(
        sequences[0].0,
        "a=T,U=1,q=2,f=32,o=z,i=1193046,s=2,v=1,c=1,r=1,m=0"
    );
}

/// The pixels `out` carries, inflated.
fn inflated(out: &[u8]) -> Vec<u8> {
    let data: String = sequences(out)
        .iter()
        .map(|(_, data)| data.as_str())
        .collect();
    let compressed = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    let mut decoded = Vec::new();
    flate2::read::ZlibDecoder::new(&compressed[..])
        .read_to_end(&mut decoded)
        .unwrap();
    decoded
}

#[test]
fn an_opaque_image_goes_as_rgb() {
    let rgba = [255, 0, 0, 255, 0, 0, 255, 255];
    let pixels = Pixels {
        width: 2,
        height: 1,
        rgba: &rgba,
    };
    let mut out = Vec::new();
    transmit(&mut out, ID, &pixels, cells(1, 1), Effort::Thorough).unwrap();
    assert_eq!(
        sequences(&out)[0].0,
        "a=T,U=1,q=2,f=24,o=z,i=1193046,s=2,v=1,c=1,r=1,m=0"
    );
    assert_eq!(inflated(&out), [255, 0, 0, 0, 0, 255]);
    assert!(is_opaque(&pixels));
}

#[test]
fn rgb_data_of_the_wrong_size_is_refused_before_the_alpha_goes() {
    // Opaque, but one pixel short of 2 x 1.
    let rgba = [255, 0, 0, 255];
    let pixels = Pixels {
        width: 2,
        height: 1,
        rgba: &rgba,
    };
    assert!(is_opaque(&pixels));
    assert!(compressed(&pixels, true, Effort::Thorough).is_err());
}

#[test]
fn an_image_with_frames_goes_as_rgba_even_when_opaque() {
    let rgba = [255, 0, 0, 255];
    let pixels = Pixels {
        width: 1,
        height: 1,
        rgba: &rgba,
    };
    let mut out = Vec::new();
    transmit_rgba(&mut out, ID, &pixels, cells(1, 1), Effort::Thorough).unwrap();
    assert!(sequences(&out)[0].0.contains(",f=32,"));
    assert_eq!(inflated(&out), rgba);
}

/// A picture of `width` × `height` pixels with some pattern in it, which
/// compresses, as a part of a page does: `alpha` for every pixel.
fn patterned(width: u32, height: u32, alpha: u8) -> Vec<u8> {
    (0..width * height)
        .flat_map(|at| {
            let (column, row) = (at % width / 6, at / width / 5);
            [
                (column * 40) as u8,
                (row * 30) as u8,
                ((column + row) % 3 * 80) as u8,
                alpha,
            ]
        })
        .collect()
}

#[test]
fn a_part_at_either_effort_inflates_to_the_same_pixels() {
    for alpha in [255, 100] {
        let rgba = patterned(120, 50, alpha);
        let pixels = Pixels {
            width: 120,
            height: 50,
            rgba: &rgba,
        };
        let sent = |effort| {
            let mut out = Vec::new();
            transmit(&mut out, ID, &pixels, cells(12, 3), effort).unwrap();
            out
        };
        let (fast, thorough) = (sent(Effort::Fast), sent(Effort::Thorough));
        // Opaque pixels go as RGB, three bytes each.
        let expected: Vec<u8> = match alpha {
            255 => (rgba.as_chunks::<4>().0.iter())
                .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
                .collect(),
            _ => rgba.clone(),
        };
        assert_eq!(inflated(&fast), expected, "fast, alpha {alpha}");
        assert_eq!(inflated(&thorough), expected, "thorough, alpha {alpha}");
        // The point of the choice: the fast level sends more bytes.
        assert!(fast.len() > thorough.len(), "alpha {alpha}");
    }
}

#[test]
fn every_way_of_sending_takes_the_effort() {
    let rgba = patterned(120, 50, 100);
    let pixels = Pixels {
        width: 120,
        height: 50,
        rgba: &rgba,
    };
    let size = |way: usize, effort| {
        let mut out = Vec::new();
        match way {
            0 => transmit_rgba(&mut out, ID, &pixels, cells(12, 3), effort),
            1 => transmit_only(&mut out, ID, &pixels, effort),
            2 => transmit_frame(&mut out, ID, &pixels, Duration::ZERO, effort),
            _ => send_shade(&mut out, 128, effort),
        }
        .unwrap();
        out.len()
    };
    // The shade is one colour, which both levels make small; the rest
    // are larger at the fast level.
    for way in 0..3 {
        let (fast, thorough) = (size(way, Effort::Fast), size(way, Effort::Thorough));
        assert!(fast > thorough, "way {way}");
    }
}

#[test]
fn sends_compressed_rgba_in_chunks() {
    let rgba = noise(64 * 64 * 4);
    let pixels = Pixels {
        width: 64,
        height: 64,
        rgba: &rgba,
    };
    let mut out = Vec::new();
    transmit(&mut out, ID, &pixels, cells(8, 4), Effort::Thorough).unwrap();
    let sequences = sequences(&out);
    assert!(sequences.len() > 1);
    let (last, rest) = sequences.split_last().unwrap();
    assert!(rest[0].0.starts_with("a=T,"));
    assert!(rest[0].0.ends_with(",m=1"));
    for (keys, data) in &rest[1..] {
        assert_eq!(keys, "q=2,m=1");
        assert_eq!(data.len(), CHUNK);
    }
    assert_eq!(rest[0].1.len(), CHUNK);
    assert_eq!(last.0, "q=2,m=0");

    let data: String = sequences.iter().map(|(_, data)| data.as_str()).collect();
    let compressed = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    let mut decoded = Vec::new();
    flate2::read::ZlibDecoder::new(&compressed[..])
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, rgba);
}

#[test]
fn a_frame_is_added_to_the_image_with_its_gap_in_milliseconds() {
    let rgba = [255, 0, 0, 255, 0, 0, 255, 128];
    let pixels = Pixels {
        width: 2,
        height: 1,
        rgba: &rgba,
    };
    let mut out = Vec::new();
    transmit_frame(
        &mut out,
        ID,
        &pixels,
        Duration::from_millis(250),
        Effort::Thorough,
    )
    .unwrap();
    let sequences = sequences(&out);
    assert_eq!(sequences.len(), 1);
    assert_eq!(
        sequences[0].0,
        "a=f,q=2,f=32,o=z,i=1193046,s=2,v=1,z=250,m=0"
    );
}

#[test]
fn a_frame_in_chunks_goes_on_as_any_data_does() {
    let rgba = noise(64 * 64 * 4);
    let pixels = Pixels {
        width: 64,
        height: 64,
        rgba: &rgba,
    };
    let mut out = Vec::new();
    transmit_frame(
        &mut out,
        ID,
        &pixels,
        Duration::from_millis(100),
        Effort::Thorough,
    )
    .unwrap();
    let sequences = sequences(&out);
    assert!(sequences.len() > 1);
    assert!(sequences[0].0.starts_with("a=f,"));
    // The chunks after the first name the action too: kitty 0.47.1
    // does not show a frame that comes in chunks with only `m`.
    let (last, rest) = sequences.split_last().unwrap();
    assert_eq!(last.0, "a=f,q=2,m=0");
    assert!(rest[1..].iter().all(|(keys, _)| keys == "a=f,q=2,m=1"));
}

#[test]
fn the_first_frame_gets_its_gap_with_the_root_frame_key() {
    let mut out = Vec::new();
    first_frame_gap(&mut out, ID, Duration::from_millis(100)).unwrap();
    assert_eq!(out, b"\x1b_Ga=a,q=2,i=1193046,r=1,z=100\x1b\\");
}

#[test]
fn controls_stop_replay_and_show() {
    let sent = |what| {
        let mut out = Vec::new();
        control(&mut out, ID, what).unwrap();
        String::from_utf8(out).unwrap()
    };
    // One play is a loop count of 2, as kitty counts; none is 1.
    assert_eq!(
        sent(Control::Replay(None)),
        "\x1b_Ga=a,q=2,i=1193046,c=1,s=3,v=1\x1b\\"
    );
    assert_eq!(sent(Control::Stop), "\x1b_Ga=a,q=2,i=1193046,s=1\x1b\\");
    assert_eq!(
        sent(Control::Replay(Some(2))),
        "\x1b_Ga=a,q=2,i=1193046,c=1,s=3,v=3\x1b\\"
    );
    assert_eq!(sent(Control::Show(3)), "\x1b_Ga=a,q=2,i=1193046,c=3\x1b\\");
}

#[test]
fn exactly_one_chunk_of_data_needs_one_sequence() {
    let payload = vec![b'A'; CHUNK];
    let mut out = Vec::new();
    write_chunks(&mut out, "a=T", &payload).unwrap();
    let expected = format!("\x1b_Ga=T,m=0;{}\x1b\\", "A".repeat(CHUNK));
    assert_eq!(String::from_utf8(out).unwrap(), expected);
}

#[test]
fn data_past_one_chunk_continues_in_the_next() {
    let payload = vec![b'A'; CHUNK + 4];
    let mut out = Vec::new();
    write_chunks(&mut out, "a=T", &payload).unwrap();
    let expected = format!(
        "\x1b_Ga=T,m=1;{}\x1b\\\x1b_Gq=2,m=0;AAAA\x1b\\",
        "A".repeat(CHUNK)
    );
    assert_eq!(String::from_utf8(out).unwrap(), expected);
}

#[test]
fn links_are_hyperlinks_round_the_placeholders_they_cover() {
    let mut out = Vec::new();
    let links = [
        LinkRun {
            columns: 1..3,
            url: "https://a.example/",
        },
        LinkRun {
            columns: 4..5,
            url: "https://b.example/\x1b]8;;evil",
        },
    ];
    write_row_linked(&mut out, ID, 2, 0..6, &links, Form::Whole).unwrap();
    let out = String::from_utf8(out).unwrap();
    let cell = |column: usize| {
        format!(
            "{PLACEHOLDER}{}{}{}",
            DIACRITICS[2], DIACRITICS[column], DIACRITICS[0]
        )
    };
    let expected = format!(
        "\x1b[38;2;18;52;86m{}\x1b]8;;https://a.example/\x1b\\{}{}\x1b]8;;\x1b\\{}\
             \x1b]8;;https://b.example/]8;;evil\x1b\\{}\x1b]8;;\x1b\\{}\x1b[39m",
        cell(0),
        cell(1),
        cell(2),
        cell(3),
        cell(4),
        cell(5)
    );
    assert_eq!(out, expected);
    // The high byte's diacritic is inside the link, before its close.
    assert!(out.contains(&format!("{}\x1b]8;;\x1b\\", cell(2))));
    // Without links it is what `write_row` writes.
    let (mut plain, mut linked) = (Vec::new(), Vec::new());
    write_row(&mut plain, ID, 2, 0..6, Form::Whole).unwrap();
    write_row_linked(&mut linked, ID, 2, 0..6, &[], Form::Whole).unwrap();
    assert_eq!(plain, linked);
}

#[test]
fn a_link_that_reaches_past_the_written_cells_is_closed_after_the_last_one() {
    let mut out = Vec::new();
    let links = [LinkRun {
        columns: 2..5,
        url: "https://a.example/",
    }];
    write_row_linked(&mut out, ID, 0, 0..3, &links, Form::Whole).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert_eq!(out.matches("\x1b]8;;https").count(), 1, "{out:?}");
    // Closed before the colour is reset, and nothing after it is linked.
    assert!(out.ends_with("\x1b]8;;\x1b\\\x1b[39m"), "{out:?}");
}

#[test]
fn deletes_an_image_with_its_data() {
    let mut out = Vec::new();
    delete(&mut out, ID).unwrap();
    // An upper-case `I` frees the data too, not only the placements.
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "\x1b_Ga=d,d=I,i=1193046,q=2\x1b\\"
    );
}

#[test]
fn rejects_pixels_that_do_not_match_the_size() {
    let pixels = Pixels {
        width: 2,
        height: 2,
        rgba: &[0; 12],
    };
    let error = transmit(&mut Vec::new(), ID, &pixels, cells(1, 1), Effort::Thorough).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn rejects_cells_without_diacritics() {
    let pixels = Pixels {
        width: 1,
        height: 1,
        rgba: &[0; 4],
    };
    for cells in [
        cells(MAX_CELLS + 1, 1),
        cells(1, MAX_CELLS + 1),
        cells(0, 1),
    ] {
        let error = transmit(&mut Vec::new(), ID, &pixels, cells, Effort::Thorough).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{cells:?}");
        let message = "an image covers 1 to 297 rows and columns";
        assert_eq!(error.to_string(), message, "{cells:?}");
    }
    let error = write_row(&mut Vec::new(), ID, MAX_CELLS, 0..1, Form::Whole).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    let error = write_row(&mut Vec::new(), ID, 0, 0..MAX_CELLS + 1, Form::Whole).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    #[allow(clippy::reversed_empty_ranges)]
    let error = write_row(&mut Vec::new(), ID, 0, 3..2, Form::Whole).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn row_of_placeholders_carries_the_id_and_positions() {
    let mut out = Vec::new();
    write_row(&mut out, ID, 2, 0..3, Form::Whole).unwrap();
    let expected = "\x1b[38;2;18;52;86m\
            \u{10EEEE}\u{030E}\u{0305}\u{0305}\
            \u{10EEEE}\u{030E}\u{030D}\u{0305}\
            \u{10EEEE}\u{030E}\u{030E}\u{0305}\
            \x1b[39m";
    assert_eq!(String::from_utf8(out).unwrap(), expected);
}

#[test]
fn a_row_can_start_in_the_middle_of_the_image() {
    let mut out = Vec::new();
    write_row(&mut out, ID, 2, 1..3, Form::Whole).unwrap();
    // The first cell names its column, the second after it.
    let expected = "\x1b[38;2;18;52;86m\
            \u{10EEEE}\u{030E}\u{030D}\u{0305}\
            \u{10EEEE}\u{030E}\u{030E}\u{0305}\
            \x1b[39m";
    assert_eq!(String::from_utf8(out).unwrap(), expected);
}

#[test]
fn the_short_form_names_the_position_on_the_first_cell_only() {
    let mut out = Vec::new();
    write_row(&mut out, ImageId(0xAB12_3456), 2, 1..4, Form::Short).unwrap();
    // Row 2, column 1 and the high byte, then bare placeholders.
    let expected = "\x1b[38;2;18;52;86m\
            \u{10EEEE}\u{030E}\u{030D}\u{1DC8}\
            \u{10EEEE}\
            \u{10EEEE}\
            \x1b[39m";
    assert_eq!(String::from_utf8(out).unwrap(), expected);
}

#[test]
fn the_short_form_names_the_position_again_where_a_link_starts_or_ends() {
    let mut out = Vec::new();
    let links = [LinkRun {
        columns: 1..3,
        url: "https://a.example/",
    }];
    write_row_linked(&mut out, ID, 2, 0..5, &links, Form::Short).unwrap();
    let whole = |column: usize| {
        format!(
            "{PLACEHOLDER}{}{}{}",
            DIACRITICS[2], DIACRITICS[column], DIACRITICS[0]
        )
    };
    let expected = format!(
        "\x1b[38;2;18;52;86m{}\x1b]8;;https://a.example/\x1b\\{}{PLACEHOLDER}\
             \x1b]8;;\x1b\\{}{PLACEHOLDER}\x1b[39m",
        whole(0),
        whole(1),
        whole(3)
    );
    assert_eq!(String::from_utf8(out).unwrap(), expected);
}

#[test]
fn last_row_and_column_use_the_last_diacritic() {
    let mut out = Vec::new();
    write_row(&mut out, ID, MAX_CELLS - 1, 0..MAX_CELLS, Form::Whole).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.ends_with("\u{10EEEE}\u{1D244}\u{1D244}\u{0305}\x1b[39m"));
    assert_eq!(out.matches(PLACEHOLDER).count(), usize::from(MAX_CELLS));
}

#[test]
fn diacritics_follow_kittys_table() {
    assert_eq!(DIACRITICS[0], '\u{0305}');
    assert_eq!(DIACRITICS[296], '\u{1D244}');
    assert!(DIACRITICS.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn id_is_stable_across_runs() {
    assert_eq!(
        ImageId::new(b"kiri", 40, 20, cells(4, 1), None).0,
        0x4A3C_F7D2
    );
}

#[test]
fn id_depends_on_content_and_size() {
    let id = ImageId::new(b"kiri", 40, 20, cells(4, 1), None);
    assert_ne!(id, ImageId::new(b"kirI", 40, 20, cells(4, 1), None));
    assert_ne!(id, ImageId::new(b"kiri", 41, 20, cells(4, 1), None));
    assert_ne!(id, ImageId::new(b"kiri", 40, 21, cells(4, 1), None));
    assert_ne!(id, ImageId::new(b"kiri", 40, 20, cells(5, 1), None));
    assert_ne!(id, ImageId::new(b"kiri", 40, 20, cells(4, 2), None));
}

#[test]
fn equal_content_gives_equal_ids() {
    let content: Vec<u8> = (0..=250).collect();
    let id = |content: &[u8]| ImageId::new(content, 40, 20, cells(4, 1), None);
    assert_eq!(id(&content), id(&content.clone()));
}

#[test]
fn a_changed_byte_anywhere_changes_the_id() {
    // Lengths on both sides of a multiple of eight: the last bytes are
    // not a whole word.
    for len in [1, 7, 8, 9, 15, 16, 17, 31, 32, 33] {
        let content = vec![0x5Au8; len];
        let id = |content: &[u8]| ImageId::new(content, 40, 20, cells(4, 1), None);
        let before = id(&content);
        for at in 0..len {
            for bit in 0..8 {
                let mut changed = content.clone();
                changed[at] ^= 1 << bit;
                assert_ne!(id(&changed), before, "bit {bit} of byte {at} of {len}");
            }
        }
    }
}

#[test]
fn content_of_another_length_gives_another_id() {
    let id = |content: &[u8]| ImageId::new(content, 40, 20, cells(4, 1), None);
    assert_ne!(id(&[0; 7]), id(&[0; 8]));
    assert_ne!(id(&[0; 8]), id(&[0; 9]));
    assert_ne!(id(&[]), id(&[0]));
}

#[test]
fn pieces_hash_as_one_wherever_they_are_cut() {
    let content: Vec<u8> = (0..70).map(|byte| byte * 3).collect();
    let whole = ImageId::new(&content, 40, 20, cells(4, 1), Some(7));
    for cut in 0..=content.len() {
        let (first, second) = content.split_at(cut);
        let pieces = [first, &[], second];
        assert_eq!(
            ImageId::of(pieces, 40, 20, cells(4, 1), Some(7)),
            whole,
            "cut at {cut}"
        );
        let three = [&content[..cut / 2], &content[cut / 2..cut], second];
        assert_eq!(ImageId::of(three, 40, 20, cells(4, 1), Some(7)), whole);
    }
}

#[test]
fn a_run_of_its_own_gives_ids_of_its_own() {
    let id = |run| ImageId::new(b"kiri", 40, 20, cells(4, 1), run);
    assert_ne!(id(Some(1)), id(None));
    assert_ne!(id(Some(1)), id(Some(2)));
    assert_eq!(id(Some(1)), id(Some(1)));
}

#[test]
fn id_folds_to_exact_values() {
    // low = (hash ^ hash >> 24 ^ hash >> 48) & 0xFFFFFF, high = 1 +
    // (hash >> 32) % 254, worked out by hand.
    for (hash, id) in [
        (0, 0x0100_0000),
        (1, 0x0100_0001),
        (0xFF_FFFF, 0x01FF_FFFF),
        (0x1_0000_0001, 0x0200_0101),
        (u64::MAX, 0x1000_FFFF),
        (0xF09A_0A2E_B649_8FD9, 0x3F43_51F5),
    ] {
        assert_eq!(fold(hash), id, "{hash:#x}");
    }
}

#[test]
fn id_has_a_high_byte_of_1_to_254_and_is_below_the_shade() {
    let mut hash: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..100_000 {
        hash = hash.wrapping_mul(0x0100_0000_01B3).rotate_left(17) ^ 0xA5A5;
        let id = fold(hash);
        assert!((1..=254).contains(&(id >> 24)), "{hash:#x} -> {id:#x}");
        assert!(id < SHADE_ID, "{hash:#x} -> {id:#x}");
    }
}

#[test]
fn a_row_of_a_32_bit_id_names_its_high_byte_on_every_cell() {
    let mut out = Vec::new();
    write_row(&mut out, ImageId(0xAB12_3456), 2, 0..3, Form::Whole).unwrap();
    let expected = "\x1b[38;2;18;52;86m\
            \u{10EEEE}\u{030E}\u{0305}\u{1DC8}\
            \u{10EEEE}\u{030E}\u{030D}\u{1DC8}\
            \u{10EEEE}\u{030E}\u{030E}\u{1DC8}\
            \x1b[39m";
    assert_eq!(String::from_utf8(out).unwrap(), expected);
}

#[test]
fn reads_a_check_answer_with_a_32_bit_id() {
    let answer = |id: &str| format!("\x1b_Gi={id},p=1263096393;OK\x1b\\").into_bytes();
    let held = |id| checked(&answer(id)).map(|(id, held)| (id.value(), held));
    assert_eq!(held("2147483648"), Some((2_147_483_648, true)));
    assert_eq!(held("4294967295"), Some((4_294_967_295, true)));
    assert_eq!(held("4294967296"), None);
}

#[test]
fn a_check_places_the_image_over_its_cells_and_takes_it_away() {
    let mut out = Vec::new();
    check(&mut out, ID, cells(4, 2)).unwrap();
    // Without q, so that the terminal answers; the delete keeps
    // the image's data and its own placement.
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "\x1b_Ga=p,U=1,i=1193046,p=1263096393,c=4,r=2\x1b\\\
             \x1b_Ga=d,d=i,i=1193046,p=1263096393,q=2\x1b\\"
    );
}

#[test]
fn reads_what_the_terminal_answers_to_a_check() {
    // As kitty 0.47.1 answers.
    let held = b"\x1b_Gi=1193046,p=1263096393;OK\x1b\\";
    assert_eq!(checked(held), Some((ID, true)));
    let missing = b"\x1b_Gi=1193046,p=1263096393;ENOENT:Put command refers to \
                        non-existent image with id: 1193046 and number: 0\x1b\\";
    assert_eq!(checked(missing), Some((ID, false)));
    // Other placements, other answers and other errors tell nothing.
    for other in [
        &b"\x1b_Gi=1193046,p=7;ENOENT:gone\x1b\\"[..],
        b"\x1b_Gi=1193046;ENOENT:gone\x1b\\",
        b"\x1b_Gi=31;OK\x1b\\",
        b"\x1b_Gi=1193046,p=1263096393;EINVAL:bad\x1b\\",
        b"\x1b_Gi=x,p=1263096393;OK\x1b\\",
        b"\x1b_Gi=0,p=1263096393;OK\x1b\\",
        b"\x1b]11;rgb:0000/0000/0000\x1b\\",
    ] {
        assert_eq!(checked(other), None, "{other:?}");
    }
}

#[test]
fn an_image_sent_apart_is_placed_at_the_cursor() {
    let rgba = [1, 2, 3, 4];
    let pixels = Pixels {
        width: 1,
        height: 1,
        rgba: &rgba,
    };
    let mut out = Vec::new();
    transmit_only(&mut out, ID, &pixels, Effort::Thorough).unwrap();
    let sent = String::from_utf8(std::mem::take(&mut out)).unwrap();
    assert!(
        sent.starts_with("\x1b_Ga=t,q=2,f=32,o=z,i=1193046,s=1,v=1,m=0;"),
        "{sent:?}"
    );
    place(&mut out, ID, 4).unwrap();
    assert_eq!(out, b"\x1b_Ga=p,q=2,i=1193046,C=1,Y=4,z=1\x1b\\");
    let short = Pixels {
        width: 2,
        height: 1,
        rgba: &rgba,
    };
    assert!(transmit_only(&mut out, ID, &short, Effort::Thorough).is_err());
}

#[test]
fn the_shade_is_placed_from_inside_its_image() {
    let mut out = Vec::new();
    place_shade(&mut out, 3, (10, 4)).unwrap();
    let placed = String::from_utf8(out).unwrap();
    assert_eq!(
        placed,
        format!("\x1b_Ga=p,q=2,i={SHADE_ID},p=3,c=10,r=4,x=2,y=2,w=124,h=124,C=1,z=-1\x1b\\")
    );
}
