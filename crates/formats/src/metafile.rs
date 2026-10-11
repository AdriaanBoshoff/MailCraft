//! Pictures in the older Windows forms an RTF body and an attachment's rendering are kept in: a
//! bitmap without a file's header ([MS-WMF] 2.2.2.9, DeviceIndependentBitmap), and a metafile
//! whose only drawing is one such bitmap ([MS-WMF], [MS-EMF]). Each becomes the bytes of a BMP
//! file, undecoded. A metafile that is a drawing of lines and filled shapes is drawn (`wmf`,
//! `emf`); one that draws anything else is not read: half a picture is worse than its place.

/// Records one metafile is searched through.
const MAX_RECORDS: usize = 4096;
/// The raster operation that copies the bitmap as it is ([MS-WMF] 2.1.1.31, SRCCOPY).
const COPY: u32 = 0x00CC_0020;

pub(crate) fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes(<[u8; 2]>::try_from(bytes).ok()?))
}

pub(crate) fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(<[u8; 4]>::try_from(bytes).ok()?))
}

pub(crate) fn size_at(data: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32_at(data, at)?).ok()
}

/// A bitmap without a file's header → a BMP file: the header such a file starts with, which says
/// where the pixels begin, and then the bitmap as it is.
pub fn dib_file(dib: &[u8]) -> Option<Vec<u8>> {
    let header = size_at(dib, 0)?;
    // The colours a bitmap of few bits looks its pixels up in, and what one of them takes.
    let (bits, used, entry, masks) = match header {
        // [MS-WMF] 2.2.2.2, BitmapCoreHeader.
        12 => (u16_at(dib, 10)?, 0, 3, 0),
        // [MS-WMF] 2.2.2.3 to 2.2.2.5, BitmapInfoHeader and the two longer ones.
        40..=124 => {
            let compression = u32_at(dib, 16)?;
            // Three masks follow the shortest header when the pixels are bit fields (BI_BITFIELDS).
            let masks = if header == 40 && compression == 3 { 12 } else { 0 };
            (u16_at(dib, 14)?, size_at(dib, 32)?, 4, masks)
        }
        _ => return None,
    };
    let colors = match bits {
        1 | 4 | 8 => {
            let all = 1usize << bits;
            if used == 0 { all } else { used.min(all) }
        }
        16 | 24 | 32 => used.min(256),
        _ => return None,
    };
    let pixels = header.checked_add(masks)?.checked_add(colors.checked_mul(entry)?)?;
    if pixels >= dib.len() {
        return None;
    }
    let (offset, total) = (pixels.checked_add(14)?, dib.len().checked_add(14)?);
    let mut file = Vec::with_capacity(total);
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&u32::try_from(total).ok()?.to_le_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&u32::try_from(offset).ok()?.to_le_bytes());
    file.extend_from_slice(dib);
    Some(file)
}

/// WMF records that set things up and draw nothing ([MS-WMF] 2.3.5 state records, 2.3.4 object
/// records, 2.3.6 escape).
const WMF_STATE: [u16; 37] = [
    0x001E, 0x0035, 0x00F7, 0x0102, 0x0103, 0x0104, 0x0105, 0x0106, 0x0107, 0x0108, 0x0127, 0x012C, 0x012D, 0x012E, 0x0142, 0x0149, 0x01F0, 0x0201,
    0x0209, 0x020A, 0x020B, 0x020C, 0x020D, 0x020E, 0x020F, 0x0211, 0x0231, 0x0234, 0x02FA, 0x02FB, 0x02FC, 0x0410, 0x0412, 0x0415, 0x0416, 0x0626,
    0x06FF,
];

/// A Windows metafile that draws one bitmap and nothing else → that bitmap as a BMP file.
pub fn wmf_bitmap(data: &[u8]) -> Option<Vec<u8>> {
    // A placeable metafile has 22 bytes before its header ([MS-WMF] 2.3.2.3).
    let mut at = if u32_at(data, 0)? == 0x9AC6_CDD7 { 22 } else { 0 };
    // [MS-WMF] 2.3.2.2, META_HEADER: in memory or on disk, and nine words long.
    if !matches!(u16_at(data, at)?, 1 | 2) || u16_at(data, at + 2)? != 9 {
        return None;
    }
    at += 18;
    let mut found: Option<&[u8]> = None;
    for _ in 0..MAX_RECORDS {
        // [MS-WMF] 2.3: a record's size in words, then what it does.
        let size = size_at(data, at)?.checked_mul(2)?;
        let function = u16_at(data, at.checked_add(4)?)?;
        if size < 6 {
            return None;
        }
        let record = data.get(at..at.checked_add(size)?)?;
        // Where the bitmap starts in each record that carries one, and where the record says how
        // it is copied or what its colours are numbers in (both must be the plain way).
        let bitmap = match function {
            // META_EOF.
            0x0000 => return found.and_then(dib_file),
            // [MS-WMF] 2.3.1.6, META_STRETCHDIB.
            0x0F43 => Some((28, u32_at(record, 6)? == COPY && u16_at(record, 10)? == 0)),
            // [MS-WMF] 2.3.1.3, META_DIBSTRETCHBLT, and 2.3.1.2, META_DIBBITBLT.
            0x0B41 => Some((26, u32_at(record, 6)? == COPY)),
            0x0940 => Some((22, u32_at(record, 6)? == COPY)),
            // [MS-WMF] 2.3.1.5, META_SETDIBTODEV.
            0x0D33 => Some((24, u16_at(record, 6)? == 0)),
            _ if WMF_STATE.contains(&function) => None,
            _ => return None,
        };
        if let Some((start, plain)) = bitmap {
            if !plain || found.is_some() {
                return None;
            }
            found = Some(record.get(start..)?);
        }
        at = at.checked_add(size)?;
    }
    None
}

/// A Windows metafile → a picture file: the one bitmap it holds, as it is; else its drawing,
/// made for showing at `want` pixels when that is known.
pub fn wmf_picture(data: &[u8], want: Option<(u32, u32)>) -> Option<Vec<u8>> {
    wmf_bitmap(data).or_else(|| crate::wmf::wmf_drawing(data, want))
}

/// EMF records that set things up and draw nothing ([MS-EMF] 2.3.11 state records, 2.3.7 object
/// creation, 2.3.8 object manipulation, 2.3.2 clipping, 2.3.12 transforms, 2.3.3 comments).
const EMF_STATE: [u32; 43] = [
    9, 10, 11, 12, 13, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 48, 49, 52, 58, 70, 75,
    82, 93, 95, 98, 99, 100, 115,
];

/// An enhanced metafile that draws one bitmap and nothing else → that bitmap as a BMP file.
pub fn emf_bitmap(data: &[u8]) -> Option<Vec<u8>> {
    // [MS-EMF] 2.3.4.2, EMR_HEADER: the first record, with " EMF" 40 bytes in.
    if u32_at(data, 0)? != 1 || u32_at(data, 40)? != 0x464D_4520 {
        return None;
    }
    let mut at = size_at(data, 4)?;
    if at < 88 || !at.is_multiple_of(4) {
        return None;
    }
    let mut found: Option<Vec<u8>> = None;
    for _ in 0..MAX_RECORDS {
        // [MS-EMF] 2.3: a record's type and its size in bytes.
        let (kind, size) = (u32_at(data, at)?, size_at(data, at.checked_add(4)?)?);
        if size < 8 || !size.is_multiple_of(4) {
            return None;
        }
        let record = data.get(at..at.checked_add(size)?)?;
        match kind {
            // EMR_EOF.
            14 => return found.as_deref().and_then(dib_file),
            // [MS-EMF] 2.3.1.7, EMR_STRETCHDIBITS: the bitmap's header and its pixels are two
            // parts of the record, each at an offset from its start.
            81 => {
                if found.is_some() || u32_at(record, 64)? != 0 || u32_at(record, 68)? != COPY {
                    return None;
                }
                let part = |at: usize| {
                    let (start, len) = (size_at(record, at)?, size_at(record, at + 4)?);
                    record.get(start..start.checked_add(len)?)
                };
                let (header, pixels) = (part(48)?, part(56)?);
                let mut dib = Vec::with_capacity(header.len().checked_add(pixels.len())?);
                dib.extend_from_slice(header);
                dib.extend_from_slice(pixels);
                found = Some(dib);
            }
            _ if EMF_STATE.contains(&kind) => {}
            _ => return None,
        }
        at = at.checked_add(size)?;
    }
    None
}

/// An enhanced metafile → a picture file: the one bitmap it holds, as it is; else its drawing,
/// made for showing at `want` pixels when that is known.
pub fn emf_picture(data: &[u8], want: Option<(u32, u32)>) -> Option<Vec<u8>> {
    emf_bitmap(data).or_else(|| crate::emf::emf_drawing(data, want))
}
