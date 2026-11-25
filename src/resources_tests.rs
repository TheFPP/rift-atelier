use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        static ID: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rift-resource-test-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn wave_compilation_preserves_samples_and_stereo_timing() {
    let dir = Fixture::new();
    let source = dir.0.join("input.wav");
    let output = dir.0.join("footstep.vsnd_c");
    let pcm = [18, 52, 241, 1, 0, 1, 144, 1];
    let mut wave = b"RIFF\x2c\0\0\0WAVEfmt \x10\0\0\0\x01\0\x02\0\x80\xbb\0\0\0\xee\x02\0\x04\0\x10\0data\x08\0\0\0".to_vec();
    wave.extend(pcm);
    fs::write(&source, wave).unwrap();
    compile_source(&dir.0, &source, "sounds/footstep.vsnd_c", &output).unwrap();
    let bytes = fs::read(output).unwrap();
    let stream = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    assert_eq!(&bytes[stream..], &pcm);
    let data = 20 + u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
    assert_eq!(
        u32::from_le_bytes(bytes[data + 8..data + 12].try_into().unwrap()),
        2
    );
    assert_eq!(
        f32::from_le_bytes(bytes[data + 12..data + 16].try_into().unwrap()),
        2.0 / 48_000.0
    );
}

#[test]
fn png_compilation_preserves_dimensions_color_and_transparency() {
    let dir = Fixture::new();
    let source = dir.0.join("input.png");
    let output = dir.0.join("color.vtex_c");
    let pixels = [1u8, 2, 3, 0, 100, 120, 160, 255];
    image::save_buffer(&source, &pixels, 2, 1, image::ColorType::Rgba8).unwrap();
    compile_source(&dir.0, &source, "materials/color.vtex_c", &output).unwrap();
    let bytes = fs::read(output).unwrap();
    let data = 20 + u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
    assert_eq!(&bytes[data + 20..data + 28], &[2, 0, 1, 0, 1, 0, 4, 2]);
    assert_eq!(&bytes[bytes.len() - 8..], &pixels);
}

#[test]
fn vpk_search_skips_preload_bytes_and_filters_file_type() {
    let dir = Fixture::new();
    let path = dir.0.join("game/dota/pak01_dir.vpk");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut tree = Vec::new();
    for ext in ["vsnd_c", "vmdl_c"] {
        tree.extend_from_slice(ext.as_bytes());
        tree.extend_from_slice(b"\0models/heroes/axe\0axe_weapon\0");
        tree.extend_from_slice(&[
            0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 65, 66, 0, 0,
        ]);
    }
    tree.push(0);
    let mut header = vec![0x34, 0x12, 0xaa, 0x55, 2, 0, 0, 0];
    header.extend_from_slice(&(tree.len() as u32).to_le_bytes());
    header.resize(28, 0);
    header.extend_from_slice(&tree);
    fs::write(path, header).unwrap();
    assert_eq!(
        find_targets(&dir.0, "vmdl_c", "AXE weapon", 10).unwrap(),
        ["models/heroes/axe/axe_weapon.vmdl_c"]
    );
}
