const MAX_SIDE: usize = 8192;
pub(crate) const MAX_BYTES: usize = 64 * 1024 * 1024;
const V5_HEADER_SIZE: usize = 124;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClipboardImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub(crate) fn encode_dibv5_with_limit(
    image: &ClipboardImage,
    max_bytes: usize,
) -> Result<Vec<u8>, &'static str> {
    let max_bytes = max_bytes.min(MAX_BYTES);
    validate_image(image.width, image.height, image.rgba.len(), max_bytes)?;
    let pixels_len = image.rgba.len();
    let mut dib = vec![0; V5_HEADER_SIZE + pixels_len];
    dib[0..4].copy_from_slice(&(V5_HEADER_SIZE as u32).to_le_bytes());
    dib[4..8].copy_from_slice(&(image.width as i32).to_le_bytes());
    dib[8..12].copy_from_slice(&(-(image.height as i32)).to_le_bytes());
    dib[12..14].copy_from_slice(&1_u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32_u16.to_le_bytes());
    dib[16..20].copy_from_slice(&3_u32.to_le_bytes());
    dib[20..24].copy_from_slice(&(pixels_len as u32).to_le_bytes());
    dib[40..44].copy_from_slice(&0x00ff_0000_u32.to_le_bytes());
    dib[44..48].copy_from_slice(&0x0000_ff00_u32.to_le_bytes());
    dib[48..52].copy_from_slice(&0x0000_00ff_u32.to_le_bytes());
    dib[52..56].copy_from_slice(&0xff00_0000_u32.to_le_bytes());
    dib[56..60].copy_from_slice(&0x7352_4742_u32.to_le_bytes());
    dib[108..112].copy_from_slice(&4_u32.to_le_bytes());
    for (rgba, bgra) in image
        .rgba
        .chunks_exact(4)
        .zip(dib[V5_HEADER_SIZE..].chunks_exact_mut(4))
    {
        bgra.copy_from_slice(&[rgba[2], rgba[1], rgba[0], rgba[3]]);
    }
    Ok(dib)
}

pub(crate) fn decode_dib_with_limit(
    dib: &[u8],
    max_bytes: usize,
) -> Result<ClipboardImage, &'static str> {
    let max_bytes = max_bytes.min(MAX_BYTES);
    if dib.len() < 40 || dib.len() > max_bytes {
        return Err("The remote clipboard image exceeds the supported size.");
    }
    let header = read_u32(dib, 0)? as usize;
    if !matches!(header, 40 | 52 | 56 | 108 | 124) || header > dib.len() {
        return Err("The remote clipboard image has an invalid DIB header.");
    }
    let width = read_i32(dib, 4)?;
    let signed_height = read_i32(dib, 8)?;
    if width <= 0 || signed_height == 0 || signed_height == i32::MIN {
        return Err("The remote clipboard image dimensions are invalid.");
    }
    let width = width as usize;
    let height = signed_height.unsigned_abs() as usize;
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err("The remote clipboard image dimensions exceed the limit.");
    }
    let bit_count = read_u16(dib, 14)?;
    let compression = read_u32(dib, 16)?;
    if !matches!(bit_count, 24 | 32) || !matches!(compression, 0 | 3) {
        return Err("The remote clipboard image format is unsupported.");
    }
    let area = width
        .checked_mul(height)
        .filter(|area| area.saturating_mul(4) <= max_bytes)
        .ok_or("The remote clipboard image dimensions exceed the limit.")?;
    let byte_count = area
        .checked_mul(4)
        .ok_or("The remote clipboard image is too large.")?;
    let mut masks = [0; 4];
    let mut data_offset = header;
    if compression == 3 {
        if header >= 52 {
            for (index, mask) in masks[..3].iter_mut().enumerate() {
                *mask = read_u32(dib, 40 + index * 4)?;
            }
            if header >= 56 {
                masks[3] = read_u32(dib, 52)?;
            }
        } else {
            if dib.len() < header + 12 {
                return Err("The remote clipboard image color masks are incomplete.");
            }
            for (index, mask) in masks[..3].iter_mut().enumerate() {
                *mask = read_u32(dib, header + index * 4)?;
            }
            data_offset += 12;
        }
        if masks[..3].contains(&0) {
            return Err("The remote clipboard image color masks are invalid.");
        }
        let valid_bits = if bit_count == 32 {
            u32::MAX
        } else {
            0x00ff_ffff
        };
        if masks.iter().any(|mask| mask & !valid_bits != 0)
            || masks[0] & masks[1] != 0
            || masks[0] & masks[2] != 0
            || masks[1] & masks[2] != 0
            || masks[3] & (masks[0] | masks[1] | masks[2]) != 0
        {
            return Err("The remote clipboard image color masks are invalid.");
        }
    }
    let bits_per_pixel = usize::from(bit_count);
    let row_bytes = width
        .checked_mul(bits_per_pixel)
        .and_then(|bits| bits.checked_add(31))
        .map(|bits| (bits / 32) * 4)
        .ok_or("The remote clipboard image row is too large.")?;
    let source_len = row_bytes
        .checked_mul(height)
        .and_then(|size| data_offset.checked_add(size))
        .filter(|end| *end <= dib.len())
        .ok_or("The remote clipboard image data is incomplete.")?;
    if source_len > max_bytes {
        return Err("The remote clipboard image exceeds the supported size.");
    }
    let mut rgba = vec![0; byte_count];
    for row in 0..height {
        let source_y = if signed_height < 0 {
            row
        } else {
            height - row - 1
        };
        let source_row = data_offset + source_y * row_bytes;
        for x in 0..width {
            let source = source_row + x * (bits_per_pixel / 8);
            let pixel = if bit_count == 24 {
                u32::from(dib[source])
                    | (u32::from(dib[source + 1]) << 8)
                    | (u32::from(dib[source + 2]) << 16)
            } else {
                u32::from_le_bytes(dib[source..source + 4].try_into().unwrap())
            };
            let channels = if compression == 3 {
                [
                    extract_channel(pixel, masks[0])?,
                    extract_channel(pixel, masks[1])?,
                    extract_channel(pixel, masks[2])?,
                    if masks[3] == 0 {
                        255
                    } else {
                        extract_channel(pixel, masks[3])?
                    },
                ]
            } else {
                [dib[source + 2], dib[source + 1], dib[source], 255]
            };
            let target = (row * width + x) * 4;
            rgba[target..target + 4].copy_from_slice(&channels);
        }
    }
    Ok(ClipboardImage {
        width,
        height,
        rgba,
    })
}

fn validate_image(
    width: usize,
    height: usize,
    byte_count: usize,
    max_bytes: usize,
) -> Result<(), &'static str> {
    let expected = width
        .checked_mul(height)
        .filter(|area| width <= MAX_SIDE && height <= MAX_SIDE && area.saturating_mul(4) <= max_bytes)
        .and_then(|area| area.checked_mul(4))
        .ok_or("The clipboard image dimensions exceed the limit.")?;
    if byte_count != expected {
        return Err("The clipboard image pixel data is invalid.");
    }
    if V5_HEADER_SIZE + byte_count > max_bytes {
        return Err("The clipboard image exceeds the configured size limit.");
    }
    Ok(())
}

fn extract_channel(pixel: u32, mask: u32) -> Result<u8, &'static str> {
    let shift = mask.trailing_zeros();
    let maximum = mask >> shift;
    if maximum == 0 || maximum & maximum.wrapping_add(1) != 0 {
        return Err("The remote clipboard image has an unsupported color mask.");
    }
    let value = (pixel & mask) >> shift;
    Ok(((u64::from(value) * 255 + u64::from(maximum) / 2) / u64::from(maximum)) as u8)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, &'static str> {
    let bytes = bytes
        .get(offset..offset + 2)
        .ok_or("The remote clipboard image header is incomplete.")?;
    Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, &'static str> {
    let bytes = bytes
        .get(offset..offset + 4)
        .ok_or("The remote clipboard image header is incomplete.")?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_i32(bytes: &[u8], offset: usize) -> Result<i32, &'static str> {
    let bytes = bytes
        .get(offset..offset + 4)
        .ok_or("The remote clipboard image header is incomplete.")?;
    Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v5_round_trip_preserves_rgba_pixels() {
        let image = ClipboardImage {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 20, 240, 128],
        };
        let dib = encode_dibv5_with_limit(&image, MAX_BYTES).unwrap();
        assert_eq!(decode_dib_with_limit(&dib, MAX_BYTES).unwrap(), image);
    }

    #[test]
    fn decodes_bottom_up_24_bit_dib_rows() {
        let mut dib = vec![0; 40 + 8];
        dib[0..4].copy_from_slice(&40_u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1_i32.to_le_bytes());
        dib[8..12].copy_from_slice(&2_i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1_u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24_u16.to_le_bytes());
        dib[40..44].copy_from_slice(&[0, 0, 255, 0]);
        dib[44..48].copy_from_slice(&[255, 0, 0, 0]);
        let image = decode_dib_with_limit(&dib, MAX_BYTES).unwrap();
        assert_eq!(image.rgba, [0, 0, 255, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn rejects_oversized_and_truncated_dibs() {
        assert!(decode_dib_with_limit(&vec![0; MAX_BYTES + 1], MAX_BYTES).is_err());
        assert!(decode_dib_with_limit(&[0; 40], MAX_BYTES).is_err());
    }

    #[test]
    fn applies_the_configured_limit_to_encode_and_decode() {
        let image = ClipboardImage {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        };
        assert!(encode_dibv5_with_limit(&image, 127).is_err());
        let encoded = encode_dibv5_with_limit(&image, 128).unwrap();
        assert!(decode_dib_with_limit(&encoded, 127).is_err());
        assert_eq!(decode_dib_with_limit(&encoded, 128).unwrap(), image);
    }

    #[test]
    fn rejects_overlapping_bitfield_masks() {
        let mut dib = encode_dibv5_with_limit(&ClipboardImage {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 255],
        }, MAX_BYTES)
        .unwrap();
        dib[44..48].copy_from_slice(&0x00ff_0000_u32.to_le_bytes());
        assert!(decode_dib_with_limit(&dib, MAX_BYTES).is_err());
    }
}
