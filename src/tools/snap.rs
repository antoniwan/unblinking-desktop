//! `--snap`: saving a frame as a PNG, and checking its transparency.

use std::error::Error;

/// Behind the eye in snap.png, so the flames are judged as on a desktop.
const SNAP_BACKDROP: [u8; 3] = [0x1d, 0x1f, 0x24];

/// Saves the frame as a PNG over a dark backdrop. The pixels arrive
/// premultiplied, so "over" is just `colour + backdrop * (1 - alpha)`.
pub(crate) fn write_snap(
    path: &str,
    (w, h): (u32, u32),
    rgba: &[u8],
) -> Result<(), Box<dyn Error>> {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for px in rgba.as_chunks::<4>().0 {
        let keep = 255 - px[3] as u32;
        for i in 0..3 {
            let v = px[i] as u32 + (SNAP_BACKDROP[i] as u32 * keep + 127) / 255;
            rgb.push(v.min(255) as u8);
        }
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgb)?;
    Ok(())
}

/// How many pixels are fully clear, partly clear and solid. A healthy frame
/// has all three: clear corners, a glowing fringe, a solid sphere.
pub(crate) fn alpha_census(rgba: &[u8]) -> (usize, usize, usize) {
    let mut counts = (0, 0, 0);
    for px in rgba.as_chunks::<4>().0 {
        match px[3] {
            0 => counts.0 += 1,
            255 => counts.2 += 1,
            _ => counts.1 += 1,
        }
    }
    counts
}
