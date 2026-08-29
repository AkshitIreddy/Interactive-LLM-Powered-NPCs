use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // tauri-build requires a Windows ICO even for `cargo check`. Generate a
    // deterministic ignored development glyph instead of committing a fake
    // release icon. A provenance-tracked approved asset at this path is never
    // overwritten.
    let manifest = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo"),
    );
    let icon = manifest.join("icons/icon.ico");
    if !icon.exists() {
        fs::write(&icon, generated_ico()).expect("write generated development icon");
    }
    let windows = tauri_build::WindowsAttributes::new().window_icon_path(icon);
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes).expect("failed to run Tauri build script");
}

fn generated_ico() -> Vec<u8> {
    const SIZE: usize = 32;
    const PIXEL_BYTES: usize = SIZE * SIZE * 4;
    const MASK_BYTES: usize = SIZE * 4;
    const IMAGE_BYTES: usize = 40 + PIXEL_BYTES + MASK_BYTES;
    let mut bytes = Vec::with_capacity(22 + IMAGE_BYTES);

    push_u16(&mut bytes, 0); // ICONDIR reserved
    push_u16(&mut bytes, 1); // ICO image type
    push_u16(&mut bytes, 1); // one image
    bytes.extend_from_slice(&[SIZE as u8, SIZE as u8, 0, 0]);
    push_u16(&mut bytes, 1); // color planes
    push_u16(&mut bytes, 32); // bits per pixel
    push_u32(&mut bytes, IMAGE_BYTES as u32);
    push_u32(&mut bytes, 22); // image offset

    push_u32(&mut bytes, 40); // BITMAPINFOHEADER size
    push_u32(&mut bytes, SIZE as u32);
    push_u32(&mut bytes, (SIZE * 2) as u32); // XOR plus AND mask height
    push_u16(&mut bytes, 1);
    push_u16(&mut bytes, 32);
    push_u32(&mut bytes, 0); // BI_RGB
    push_u32(&mut bytes, PIXEL_BYTES as u32);
    bytes.extend_from_slice(&[0; 16]); // resolution and palette fields

    // DIB pixel rows are bottom-up and BGRA ordered.
    for source_y in (0..SIZE).rev() {
        for x in 0..SIZE {
            let left_stem = (8..=11).contains(&x) && (7..=24).contains(&source_y);
            let right_stem = (20..=23).contains(&x) && (7..=24).contains(&source_y);
            let diagonal = (8..=23).contains(&x)
                && (7..=24).contains(&source_y)
                && ((x as isize - 8) - (source_y as isize - 7)).unsigned_abs() <= 2;
            if left_stem || right_stem || diagonal {
                bytes.extend_from_slice(&[195, 214, 99, 255]);
            } else {
                bytes.extend_from_slice(&[34, 31, 25, 255]);
            }
        }
    }
    bytes.extend(vec![0; MASK_BYTES]);
    bytes
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
