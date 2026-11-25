use super::*;
use image::{GenericImageView, Rgba, RgbaImage};
use std::io::Write;
use zip::write::SimpleFileOptions;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("rift-previews-{}", crate::store::uid()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn png(color: [u8; 4]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    RgbaImage::from_pixel(4, 3, Rgba(color))
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}
fn texture_dxt5() -> Vec<u8> {
    let mut raw = vec![0; 32 + 40 + 16];
    raw[..4].copy_from_slice(&72u32.to_le_bytes());
    raw[4..6].copy_from_slice(&12u16.to_le_bytes());
    raw[6..8].copy_from_slice(&1u16.to_le_bytes());
    raw[8..12].copy_from_slice(&8u32.to_le_bytes());
    raw[12..16].copy_from_slice(&1u32.to_le_bytes());
    raw[16..20].copy_from_slice(b"DATA");
    raw[20..24].copy_from_slice(&12u32.to_le_bytes());
    raw[24..28].copy_from_slice(&40u32.to_le_bytes());
    raw[32..34].copy_from_slice(&1u16.to_le_bytes());
    raw[52..54].copy_from_slice(&4u16.to_le_bytes());
    raw[54..56].copy_from_slice(&4u16.to_le_bytes());
    raw[56..58].copy_from_slice(&1u16.to_le_bytes());
    raw[58] = 2;
    raw[59] = 1;
    raw[72] = 255;
    raw[80..82].copy_from_slice(&0xf800u16.to_le_bytes());
    raw
}
#[test]
fn bundled_portrait_is_decoded_and_verified_as_an_icon() {
    let d = Temp::new();
    let texture = d.0.join("portrait.vtex_c");
    fs::write(&texture, texture_dxt5()).unwrap();
    let target = "panorama/images/heroes/hero_png.vtex_c";
    let package = d.0.join("hero.vpk");
    packages::export(
        &[packages::Input::Loose {
            source: texture,
            target: target.into(),
        }],
        &package,
    )
    .unwrap();
    assert!(packages::read_member(&package, target, 8).is_err());
    let (cover, source) = discover(&package, &package, &d.0).unwrap().unwrap();
    let image = image::open(cover).unwrap().to_rgba8();
    assert_eq!(source, "Игровая иконка из пакета");
    assert_eq!(image.dimensions(), (4, 4));
    assert_eq!(image.get_pixel(1, 2).0, [255, 0, 0, 255]);
}
#[test]
fn zip_variants_get_their_own_screenshot_and_ignore_the_other_variant() {
    let d = Temp::new();
    let archive = d.0.join("variants.zip");
    let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
    let a = png([3, 129, 40, 255]);
    let b = png([220, 12, 99, 255]);
    for (name, picture) in [("green", a), ("red", b)] {
        zip.start_file(
            format!("{name}/pak02_dir.vpk"),
            SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"VPK placeholder for thumbnail selection")
            .unwrap();
        zip.start_file(format!("{name}/preview.png"), SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&picture).unwrap();
    }
    zip.finish().unwrap();
    let (cover, caption) = discover(&archive, Path::new("002-red-pak02_dir.vpk"), &d.0)
        .unwrap()
        .unwrap();
    assert_eq!(caption, "Изображение из ZIP");
    assert_eq!(
        image::open(cover).unwrap().get_pixel(2, 1).0,
        [220, 12, 99, 255]
    );
}
#[test]
fn attached_webp_is_copied_as_a_bounded_cover_and_survives_source_removal() {
    let d = Temp::new();
    let original = d.0.join("hero.webp");
    RgbaImage::from_pixel(1200, 400, Rgba([24, 92, 132, 255]))
        .save(&original)
        .unwrap();
    let result = attach(&original, &d.0).unwrap();
    fs::remove_file(original).unwrap();
    let image = image::open(result).unwrap();
    assert_eq!(image.dimensions(), (768, 256));
    assert_eq!(image.get_pixel(13, 18).0, [24, 92, 132, 255]);
}
#[test]
fn standalone_model_uses_its_own_sibling_image() {
    let d = Temp::new();
    let model = d.0.join("abc.vmdl_c");
    fs::write(&model, b"opaque").unwrap();
    fs::write(d.0.join("unrelated.jpg"), png([9, 20, 5, 255])).unwrap();
    assert!(discover(&model, &model, &d.0).unwrap().is_none());
    fs::write(d.0.join("abc_preview.png"), png([9, 20, 5, 255])).unwrap();
    let (image, source) = discover(&model, &model, &d.0).unwrap().unwrap();
    assert_eq!(source, "Изображение рядом с модом");
    assert_eq!(
        image::open(image).unwrap().get_pixel(2, 1).0,
        [9, 20, 5, 255]
    );
}
