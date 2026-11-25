use image::{DynamicImage, RgbaImage};

fn u16_at(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?) as usize)
}
fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
}
fn relative(bytes: &[u8], at: usize) -> Option<usize> {
    at.checked_add(u32_at(bytes, at)?)
}
fn level_bytes(format: u8, w: usize, h: usize) -> Option<usize> {
    match format {
        1 => Some(w.div_ceil(4) * h.div_ceil(4) * 8),
        2 | 20 => Some(w.div_ceil(4) * h.div_ceil(4) * 16),
        4 | 28 => Some(w * h * 4),
        _ => None,
    }
}

fn pixels(format: u8, w: usize, h: usize, data: &[u8]) -> Option<RgbaImage> {
    if data.len() != level_bytes(format, w, h)? {
        return None;
    }
    let bytes = match format {
        4 => data.to_vec(),
        28 => {
            let mut rgba = data.to_vec();
            for pixel in rgba.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            rgba
        }
        1 | 2 | 20 => {
            let mut rgba = vec![0; w * h * 4];
            let block_bytes = if format == 1 { 8 } else { 16 };
            let columns = w.div_ceil(4);
            let mut block = [0; 64];
            for (i, input) in data.chunks_exact(block_bytes).enumerate() {
                match format {
                    1 => bcdec_rs::bc1(input, &mut block, 16),
                    2 => bcdec_rs::bc3(input, &mut block, 16),
                    _ => bcdec_rs::bc7(input, &mut block, 16),
                }
                let x = (i % columns) * 4;
                let y = (i / columns) * 4;
                let row_bytes = (w - x).min(4) * 4;
                for row in 0..(h - y).min(4) {
                    let dst = ((y + row) * w + x) * 4;
                    rgba[dst..dst + row_bytes]
                        .copy_from_slice(&block[row * 16..row * 16 + row_bytes]);
                }
            }
            rgba
        }
        _ => return None,
    };
    RgbaImage::from_raw(w as u32, h as u32, bytes)
}

fn uses_ycocg(block: &[u8]) -> Option<bool> {
    let tag = b"Texture Compiler Version Image YCoCg Conversion";
    let has_tag = |data: &[u8]| data.windows(tag.len()).any(|s| s == tag);
    if block.get(1..4) != Some(b"3VK") || u32_at(block, 20)? == 0 {
        return Some(has_tag(block));
    }
    // RED2 stores compiler dependencies in a binary KV3 string table. Its LZ4
    // section must be decompressed before matching a compiler transform.
    if u32_at(block, 20)? != 1 {
        return None;
    }
    let (header, raw, packed) = match block[0] {
        1 => (40, u32_at(block, 36)?, block.len().checked_sub(40)?),
        2 | 3 => (64, u32_at(block, 48)?, u32_at(block, 52)?),
        4 => (72, u32_at(block, 48)?, u32_at(block, 52)?),
        5 => (120, u32_at(block, 72)?, u32_at(block, 76)?),
        _ => return None,
    };
    if raw > 4 * 1024 * 1024 {
        return None;
    }
    let table = lz4_flex::block::decompress(block.get(header..header + packed)?, raw).ok()?;
    Some(has_tag(&table))
}

fn restore_ycocg(image: &mut RgbaImage) {
    for p in image.pixels_mut() {
        let [r, g, b, a] = p.0;
        let scale = f32::from(b) / 8. + 1.;
        let co = (f32::from(r) - 128.) / scale;
        let cg = (f32::from(g) - 128.) / scale;
        let y = f32::from(a);
        p.0 = [
            (y + co - cg).round() as u8,
            (y + cg).round() as u8,
            (y - co - cg).round() as u8,
            255,
        ];
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn reads_lz4_edit_metadata_before_restoring_portrait_colour() {
        let text = b"Texture Compiler Version Image YCoCg Conversion";
        let packed = lz4_flex::block::compress(text);
        let mut kv = vec![0; 120];
        kv[..4].copy_from_slice(b"\x053VK");
        kv[20..24].copy_from_slice(&1u32.to_le_bytes());
        kv[72..76].copy_from_slice(&(text.len() as u32).to_le_bytes());
        kv[76..80].copy_from_slice(&(packed.len() as u32).to_le_bytes());
        kv.extend(packed);
        assert_eq!(uses_ycocg(&kv), Some(true));
        let mut image = RgbaImage::from_raw(1, 1, vec![168, 112, 8, 110]).unwrap();
        restore_ycocg(&mut image);
        assert_eq!(image.as_raw(), &[138, 102, 98, 255]);
    }
}

/// Decode one 2D preview mip. Unsupported layouts can fall back to an attached picture.
pub(super) fn decode(bytes: &[u8]) -> Option<DynamicImage> {
    if u16_at(bytes, 4)? != 12 || u16_at(bytes, 6)? == 0 {
        return None;
    }
    let table = relative(bytes, 8)?;
    let count = u32_at(bytes, 12)?;
    if count > 128 {
        return None;
    }
    let mut data_block = None;
    let mut edit_info = None;
    for i in 0..count {
        let entry = table + i * 12;
        let kind = bytes.get(entry..entry + 4)?;
        if kind == b"DATA" {
            let offset = relative(bytes, entry + 4)?;
            let size = u32_at(bytes, entry + 8)?;
            data_block = Some((
                bytes.get(offset..offset + size)?,
                bytes.get(offset + size..)?,
            ));
        } else if kind == b"REDI" || kind == b"RED2" {
            let offset = relative(bytes, entry + 4)?;
            edit_info = Some(bytes.get(offset..offset + u32_at(bytes, entry + 8)?)?);
        }
    }
    let (data, stream) = data_block?;
    let width = u16_at(data, 20)?;
    let height = u16_at(data, 22)?;
    let depth = u16_at(data, 24)?;
    let flags = u16_at(data, 2)?;
    let format = *data.get(26)?;
    let mips = *data.get(27)? as usize;
    if u16_at(data, 0)? != 1
        || !(1..=8192).contains(&width)
        || !(1..=8192).contains(&height)
        || depth != 1
        || flags & 0x70 != 0
        || !(1..=16).contains(&mips)
    {
        return None;
    }
    if matches!(format, 15..=18 | 29 | 30) {
        return super::decode_raster(stream, None).ok();
    }
    let mut display_width = width;
    let mut display_height = height;
    let mut sizes = None;
    let mut compression = 0;
    let extras = u32_at(data, 36)?;
    if extras > 32 {
        return None;
    }
    let ext_table = relative(data, 32)?;
    for i in 0..extras {
        let entry = ext_table + i * 12;
        let kind = u32_at(data, entry)?;
        let start = relative(data, entry + 4)?;
        let ext = data.get(start..start + u32_at(data, entry + 8)?)?;
        match kind {
            3 => {
                let w = u16_at(ext, 2)?;
                let h = u16_at(ext, 4)?;
                if w > 0 && w <= width && (w != 1 || width == 4) {
                    display_width = w;
                }
                if h > 0 && h <= height && (h != 1 || height == 4) {
                    display_height = h;
                }
            }
            4 => {
                compression = u32_at(ext, 0)?;
                if u32_at(ext, 8)? != mips {
                    return None;
                }
                let s = relative(ext, 4)?;
                sizes = Some(ext.get(s..s + mips * 4)?);
            }
            _ => {}
        }
    }
    let mut selected = 0;
    while selected + 1 < mips && (width >> selected).max(height >> selected) > 1024 {
        selected += 1;
    }
    let w = (width >> selected).max(1);
    let h = (height >> selected).max(1);
    if w * h > 8_388_608 {
        return None;
    }
    let mut offset = 0;
    for level in (selected..mips).rev() {
        let len = level_bytes(format, (width >> level).max(1), (height >> level).max(1))?;
        let stored = match sizes {
            Some(s) => u32_at(s, 4 * level)?.min(len),
            None => len,
        };
        if stored == 0 {
            return None;
        }
        let raw = stream.get(offset..offset + stored)?;
        offset += stored;
        if level != selected {
            continue;
        }
        let decoded;
        let source = if stored < len {
            if compression != 1 {
                return None;
            }
            decoded = lz4_flex::block::decompress(raw, len).ok()?;
            &decoded
        } else {
            raw
        };
        let mut image = pixels(format, w, h, source)?;
        if format == 2
            && let Some(info) = edit_info
            && uses_ycocg(info)?
        {
            restore_ycocg(&mut image);
        }
        let img = DynamicImage::ImageRgba8(image);
        let dw = (display_width >> level).max(1);
        let dh = (display_height >> level).max(1);
        return Some(if dw == w && dh == h {
            img
        } else {
            img.crop_imm(0, 0, dw as u32, dh as u32)
        });
    }
    None
}
