//! A font made here, of plain boxes: one box for every character asked for. It lets tests of
//! layout and of PDF writing run the same on every machine, whatever fonts are installed, with
//! widths that are known beforehand. Nothing is shown to a person in it.
//!
//! The file is laid out as the OpenType specification gives it ("Font file", and the tables
//! `cmap`, `glyf`, `head`, `hhea`, `hmtx`, `loca`, `maxp`, `OS/2`; for pairs that stand closer
//! together, `kern` or `GPOS`; for characters drawn with other glyphs than their own, `GSUB`).

/// Font units to the em.
pub const UNITS: u16 = 1000;
/// How far every character but the space moves on, in font units.
pub const ADVANCE: u16 = 500;
/// How far a space moves on.
pub const SPACE: u16 = 250;
pub const ASCENT: i16 = 800;
pub const DESCENT: i16 = -200;
/// A character that is drawn by pointing at another glyph (the first box), when asked for.
pub const COMPOSED: char = '\u{c9}';

/// A TrueType font with a box for each of `chars`. A space is an empty glyph. Glyph 0 is the
/// empty "not defined" glyph; the characters follow in the order given, from glyph 1.
pub fn font(chars: &str) -> Vec<u8> {
    font_with(chars, 0)
}

/// As [`font`], with the embedding permissions (`fsType` of the `OS/2` table) given.
pub fn font_with(chars: &str, fs_type: u16) -> Vec<u8> {
    build(chars, fs_type, Vec::new())
}

/// As [`font`], with more tables, each written as given: for tables a test makes itself.
pub fn font_and(chars: &str, tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    build(chars, 0, tables)
}

/// The characters a font is made for, each once.
fn listed(chars: &str) -> Vec<char> {
    let mut listed: Vec<char> = Vec::new();
    for ch in chars.chars().take(4096) {
        if !listed.contains(&ch) {
            listed.push(ch);
        }
    }
    listed
}

/// As [`font`], with pairs of characters that stand closer together (or further apart) by so
/// many font units: in a `kern` table, or as the `kern` feature of a `GPOS` table.
pub fn font_kerned(chars: &str, pairs: &[(char, char, i16)], gpos: bool) -> Vec<u8> {
    let listed = listed(chars);
    let glyph = |ch: char| listed.iter().position(|c| *c == ch).and_then(|at| u16::try_from(at + 1).ok());
    let mut by_glyph: Vec<(u16, u16, i16)> = pairs.iter().filter_map(|(a, b, v)| Some((glyph(*a)?, glyph(*b)?, *v))).collect();
    by_glyph.sort_unstable();
    by_glyph.dedup_by_key(|p| (p.0, p.1));
    let count = u16::try_from(by_glyph.len()).unwrap_or(0);
    let mut table: Vec<u8> = Vec::new();
    let put = |table: &mut Vec<u8>, values: &[u16]| values.iter().for_each(|v| table.extend(v.to_be_bytes()));
    if !gpos {
        // `kern`, version 0, with one subtable of format 0 that kerns along the line.
        let power = 15u16.saturating_sub(count.max(1).leading_zeros() as u16);
        let range = 6u16 << power;
        put(&mut table, &[0, 1, 0, 14 + count * 6, 0x0001, count, range, power, (count * 6).saturating_sub(range)]);
        for (a, b, v) in &by_glyph {
            put(&mut table, &[*a, *b, *v as u16]);
        }
        return build(chars, 0, vec![(*b"kern", table)]);
    }
    // `GPOS` 1.0: one script (`DFLT`) whose default language system has one feature (`kern`)
    // with one lookup of type 2 and one subtable of format 1.
    let mut firsts: Vec<u16> = by_glyph.iter().map(|p| p.0).collect();
    firsts.dedup();
    let sets = u16::try_from(firsts.len()).unwrap_or(0);
    put(&mut table, &[1, 0, 10, 30, 44]);
    // Script list: one record, the script, its default language system.
    put(&mut table, &[1]);
    table.extend(b"DFLT");
    put(&mut table, &[8, 4, 0, 0, 0xFFFF, 1, 0]);
    // Feature list.
    put(&mut table, &[1]);
    table.extend(b"kern");
    put(&mut table, &[8, 0, 1, 0]);
    // Lookup list, and the lookup.
    put(&mut table, &[1, 4, 2, 0, 1, 8]);
    // The subtable: the amount is the first glyph's way on (value format 0x0004).
    let head = 10 + sets * 2;
    let mut offsets: Vec<u16> = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    for first in &firsts {
        offsets.push(head + u16::try_from(body.len()).unwrap_or(0));
        let seconds: Vec<&(u16, u16, i16)> = by_glyph.iter().filter(|p| p.0 == *first).collect();
        put(&mut body, &[u16::try_from(seconds.len()).unwrap_or(0)]);
        for (_, b, v) in seconds {
            put(&mut body, &[*b, *v as u16]);
        }
    }
    put(&mut table, &[1, head + u16::try_from(body.len()).unwrap_or(0), 0x0004, 0, sets]);
    put(&mut table, &offsets);
    table.extend(body);
    // Coverage, format 1: the first glyphs in order.
    put(&mut table, &[1, sets]);
    put(&mut table, &firsts);
    build(chars, 0, vec![(*b"GPOS", table)])
}

/// A `GSUB` or `GPOS` table, version 1.0, of features that each have one lookup with one
/// subtable: (the feature, the lookup's type, the subtable). One script, `DFLT`, whose default
/// language system has them all ("OpenType Layout Common Table Formats").
fn layout_table(mut features: Vec<([u8; 4], u16, Vec<u8>)>) -> Vec<u8> {
    features.sort_by_key(|f| f.0);
    let put = |table: &mut Vec<u8>, values: &[u16]| values.iter().for_each(|v| table.extend(v.to_be_bytes()));
    let n = u16::try_from(features.len()).unwrap_or(0);
    let mut table: Vec<u8> = Vec::new();
    let (scripts, list) = (18 + 2 * n, 2 + 12 * n);
    put(&mut table, &[1, 0, 10, 10 + scripts, 10 + scripts + list]);
    put(&mut table, &[1]);
    table.extend(b"DFLT");
    put(&mut table, &[8, 4, 0, 0, 0xFFFF, n]);
    put(&mut table, &(0..n).collect::<Vec<u16>>());
    put(&mut table, &[n]);
    for (i, (tag, ..)) in (0..n).zip(&features) {
        table.extend(tag);
        put(&mut table, &[2 + 6 * n + 6 * i]);
    }
    for i in 0..n {
        put(&mut table, &[0, 1, i]);
    }
    put(&mut table, &[n]);
    let mut at = 2 + 2 * n;
    for (_, _, subtable) in &features {
        put(&mut table, &[at]);
        at += 8 + u16::try_from(subtable.len()).unwrap_or(0);
    }
    for (_, kind, subtable) in features {
        put(&mut table, &[kind, 0, 1, 8]);
        table.extend(subtable);
    }
    table
}

/// As [`font`], with tables of its own to shape text by. `drawn` is (a feature, the characters
/// read, the character whose glyph they are drawn with): a `GSUB` feature with one lookup of
/// type 4, so `("liga", "fi", X)` draws f and i as the glyph of X, and `("init", "b", Y)` draws
/// b as the glyph of Y where the feature is asked for. `put` is (a character, to the right, up)
/// in font units: where its glyph is drawn away from its place, as the `kern` feature of a
/// `GPOS` table (a lookup of type 1).
pub fn font_shaped(chars: &str, drawn: &[(&str, &str, char)], moved: &[(char, i16, i16)]) -> Vec<u8> {
    let listed = listed(chars);
    let glyph = |ch: char| listed.iter().position(|c| *c == ch).and_then(|at| u16::try_from(at + 1).ok());
    let put = |table: &mut Vec<u8>, values: &[u16]| values.iter().for_each(|v| table.extend(v.to_be_bytes()));
    let len = |bytes: &Vec<u8>| u16::try_from(bytes.len()).unwrap_or(0);
    let mut tags: Vec<[u8; 4]> = Vec::new();
    for (tag, ..) in drawn {
        if let Ok(tag) = <[u8; 4]>::try_from(tag.as_bytes())
            && !tags.contains(&tag)
        {
            tags.push(tag);
        }
    }
    let mut gsub: Vec<([u8; 4], u16, Vec<u8>)> = Vec::new();
    for tag in tags {
        // (the glyphs read, the glyph drawn), by the first glyph read.
        let mut rules: Vec<(Vec<u16>, u16)> = drawn
            .iter()
            .filter(|(t, ..)| t.as_bytes() == tag)
            .filter_map(|(_, from, to)| Some((from.chars().map(glyph).collect::<Option<Vec<u16>>>().filter(|g| !g.is_empty())?, glyph(*to)?)))
            .collect();
        rules.sort_by_key(|r| r.0.first().copied());
        let mut firsts: Vec<u16> = rules.iter().filter_map(|r| r.0.first().copied()).collect();
        firsts.dedup();
        let sets = u16::try_from(firsts.len()).unwrap_or(0);
        let mut offsets: Vec<u16> = Vec::new();
        let mut body: Vec<u8> = Vec::new();
        for first in &firsts {
            offsets.push(6 + 2 * sets + len(&body));
            let of: Vec<&(Vec<u16>, u16)> = rules.iter().filter(|r| r.0.first() == Some(first)).collect();
            let count = u16::try_from(of.len()).unwrap_or(0);
            let mut set: Vec<u8> = Vec::new();
            let mut starts: Vec<u16> = Vec::new();
            for (from, to) in of {
                starts.push(2 + 2 * count + len(&set));
                put(&mut set, &[*to, u16::try_from(from.len()).unwrap_or(1)]);
                put(&mut set, from.get(1..).unwrap_or_default());
            }
            put(&mut body, &[count]);
            put(&mut body, &starts);
            body.extend(set);
        }
        let mut sub: Vec<u8> = Vec::new();
        put(&mut sub, &[1, 6 + 2 * sets + len(&body), sets]);
        put(&mut sub, &offsets);
        sub.extend(body);
        put(&mut sub, &[1, sets]);
        put(&mut sub, &firsts);
        gsub.push((tag, 4, sub));
    }
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    if !gsub.is_empty() {
        tables.push((*b"GSUB", layout_table(gsub)));
    }
    let mut places: Vec<(u16, i16, i16)> = moved.iter().filter_map(|(ch, dx, dy)| Some((glyph(*ch)?, *dx, *dy))).collect();
    places.sort_unstable();
    places.dedup_by_key(|p| p.0);
    if !places.is_empty() {
        // Format 2, each glyph with where it is put (value format 0x0003: across and up).
        let count = u16::try_from(places.len()).unwrap_or(0);
        let mut sub: Vec<u8> = Vec::new();
        put(&mut sub, &[2, 8 + 4 * count, 0x0003, count]);
        for (_, dx, dy) in &places {
            put(&mut sub, &[*dx as u16, *dy as u16]);
        }
        put(&mut sub, &[1, count]);
        put(&mut sub, &places.iter().map(|p| p.0).collect::<Vec<u16>>());
        tables.push((*b"GPOS", layout_table(vec![(*b"kern", 1, sub)])));
    }
    build(chars, 0, tables)
}

fn build(chars: &str, fs_type: u16, more: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    let listed = listed(chars);
    let count = u16::try_from(listed.len() + 1).unwrap_or(u16::MAX);

    let mut glyf: Vec<u8> = Vec::new();
    let mut loca: Vec<u8> = Vec::new();
    let mut hmtx: Vec<u8> = Vec::new();
    // Glyph 0: nothing drawn.
    loca.extend(0u32.to_be_bytes());
    hmtx.extend(ADVANCE.to_be_bytes());
    hmtx.extend(0i16.to_be_bytes());
    for ch in &listed {
        loca.extend(u32::try_from(glyf.len()).unwrap_or(u32::MAX).to_be_bytes());
        match *ch {
            ' ' => {}
            COMPOSED => {
                // One part: glyph 1 where it stands ("glyf", composite glyph description).
                for v in [-1i16, 50, 0, 450, 700] {
                    glyf.extend(v.to_be_bytes());
                }
                glyf.extend(0x0002u16.to_be_bytes());
                glyf.extend(1u16.to_be_bytes());
                glyf.extend([0u8, 0u8]);
            }
            _ => {
                for v in [1i16, 50, 0, 450, 700] {
                    glyf.extend(v.to_be_bytes());
                }
                glyf.extend(3u16.to_be_bytes());
                glyf.extend(0u16.to_be_bytes());
                glyf.extend([1u8; 4]);
                for v in [50i16, 0, 400, 0, 0, 700, 0, -700] {
                    glyf.extend(v.to_be_bytes());
                }
            }
        }
        while !glyf.len().is_multiple_of(4) {
            glyf.push(0);
        }
        hmtx.extend(if *ch == ' ' { SPACE } else { ADVANCE }.to_be_bytes());
        hmtx.extend(if *ch == ' ' { 0i16 } else { 50i16 }.to_be_bytes());
    }
    loca.extend(u32::try_from(glyf.len()).unwrap_or(u32::MAX).to_be_bytes());

    let mut head: Vec<u8> = Vec::new();
    head.extend(0x0001_0000u32.to_be_bytes());
    head.extend(0x0001_0000u32.to_be_bytes());
    head.extend(0u32.to_be_bytes());
    head.extend(0x5F0F_3CF5u32.to_be_bytes());
    head.extend(0u16.to_be_bytes());
    head.extend(UNITS.to_be_bytes());
    head.extend([0u8; 16]);
    for v in [0i16, DESCENT, 500, ASCENT] {
        head.extend(v.to_be_bytes());
    }
    head.extend(0u16.to_be_bytes());
    head.extend(8u16.to_be_bytes());
    head.extend(2i16.to_be_bytes());
    // Long offsets in `loca`.
    head.extend(1i16.to_be_bytes());
    head.extend(0i16.to_be_bytes());

    let mut hhea: Vec<u8> = Vec::new();
    hhea.extend(0x0001_0000u32.to_be_bytes());
    for v in [ASCENT, DESCENT, 0] {
        hhea.extend(v.to_be_bytes());
    }
    hhea.extend(ADVANCE.to_be_bytes());
    for v in [0i16, 50, 450, 1, 0, 0, 0, 0, 0, 0, 0] {
        hhea.extend(v.to_be_bytes());
    }
    hhea.extend(count.to_be_bytes());

    let mut maxp: Vec<u8> = Vec::new();
    maxp.extend(0x0001_0000u32.to_be_bytes());
    maxp.extend(count.to_be_bytes());
    for v in [4u16, 1, 4, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1] {
        maxp.extend(v.to_be_bytes());
    }

    // One group per character ("cmap", format 12), in the order of the characters' numbers.
    let mut mapped: Vec<(u32, u32)> = listed.iter().enumerate().map(|(i, ch)| (u32::from(*ch), u32::try_from(i + 1).unwrap_or(0))).collect();
    mapped.sort_unstable();
    let mut cmap: Vec<u8> = Vec::new();
    cmap.extend(0u16.to_be_bytes());
    cmap.extend(1u16.to_be_bytes());
    cmap.extend(3u16.to_be_bytes());
    cmap.extend(10u16.to_be_bytes());
    cmap.extend(12u32.to_be_bytes());
    cmap.extend(12u16.to_be_bytes());
    cmap.extend(0u16.to_be_bytes());
    cmap.extend(u32::try_from(16 + mapped.len() * 12).unwrap_or(u32::MAX).to_be_bytes());
    cmap.extend(0u32.to_be_bytes());
    cmap.extend(u32::try_from(mapped.len()).unwrap_or(0).to_be_bytes());
    for (ch, glyph) in &mapped {
        cmap.extend(ch.to_be_bytes());
        cmap.extend(ch.to_be_bytes());
        cmap.extend(glyph.to_be_bytes());
    }

    // Version 0 of `OS/2`: 78 bytes, of which only the permissions are told.
    let mut os2 = vec![0u8; 78];
    if let Some(at) = os2.get_mut(8..10) {
        at.copy_from_slice(&fs_type.to_be_bytes());
    }

    let mut tables = vec![
        (*b"OS/2", os2),
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
    ];
    tables.extend(more);
    tables.sort_by_key(|(tag, _)| *tag);
    file(&tables)
}

/// A font file of `tables`, which are given in the order of their tags.
fn file(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let count = u16::try_from(tables.len()).unwrap_or(0);
    let mut out: Vec<u8> = Vec::new();
    out.extend(0x0001_0000u32.to_be_bytes());
    out.extend(count.to_be_bytes());
    let power = 15u16.saturating_sub(count.max(1).leading_zeros() as u16);
    let range = 16u16 << power;
    out.extend(range.to_be_bytes());
    out.extend(power.to_be_bytes());
    out.extend(count.saturating_mul(16).saturating_sub(range).to_be_bytes());
    let mut at = 12 + tables.len() * 16;
    for (tag, data) in tables {
        out.extend(tag);
        out.extend(0u32.to_be_bytes());
        out.extend(u32::try_from(at).unwrap_or(u32::MAX).to_be_bytes());
        out.extend(u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
        at += data.len().div_ceil(4) * 4;
    }
    for (_, data) in tables {
        out.extend(data);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    out
}
