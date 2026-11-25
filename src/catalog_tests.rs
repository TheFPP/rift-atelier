use super::*;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

fn scratch() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("dota-archive-test-{}-{stamp}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    dir
}

fn archive(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join("download.zip");
    let mut writer = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    for (name, data) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap();
    path
}

// A minimal VPK v1 containing an empty directory tree.
const VPK: &[u8] = &[0x34, 0x12, 0xaa, 0x55, 1, 0, 0, 0, 1, 0, 0, 0, 0];

#[test]
fn imports_only_vpks_and_keeps_same_named_variants_separate() {
    let dir = scratch();
    let zip = archive(
        &dir,
        &[
            ("green/Hero.VPK", VPK),
            ("red/Hero.VPK", VPK),
            ("helper.exe", b"ignored"),
        ],
    );
    let paths = extract_vpks(&zip, &dir.join("cache")).unwrap();
    assert_eq!(paths.len(), 2);
    assert_ne!(paths[0], paths[1]);
    for path in &paths {
        assert_eq!(fs::read(path).unwrap(), VPK);
    }
    assert_eq!(fs::read_dir(paths[0].parent().unwrap()).unwrap().count(), 2);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn traversal_and_invalid_vpks_leave_no_partially_imported_files() {
    let dir = scratch();
    let cache = dir.join("cache");
    for (name, data) in [
        ("../escaped.vpk", VPK),
        ("broken.vpk", b"not a package".as_slice()),
    ] {
        let zip = archive(&dir, &[("good.vpk", VPK), (name, data)]);
        assert!(extract_vpks(&zip, &cache).is_err());
        assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
        assert!(!dir.join("escaped.vpk").exists());
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn presents_recent_archives_without_partial_downloads_or_subdirectories() {
    let dir = scratch();
    let ready = archive(&dir, &[("pak02_dir.vpk", VPK)]);
    fs::rename(ready, dir.join("old.zip")).unwrap();
    archive(&dir, &[("readme.txt", b"only documentation")]);
    fs::write(dir.join("new.VPK"), VPK).unwrap();
    fs::write(dir.join("broken.zip"), b"PK\x03\x04incomplete").unwrap();
    fs::write(dir.join("fake.vpk"), b"invalid resource signature").unwrap();
    for name in ["empty.zip", "empty.vpk", "new.VPK.part", "tool.exe"] {
        fs::write(dir.join(name), []).unwrap();
    }
    fs::OpenOptions::new()
        .write(true)
        .open(dir.join("old.zip"))
        .unwrap()
        .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000))
        .unwrap();
    fs::create_dir(dir.join("folder.vpk")).unwrap();
    assert_eq!(
        recent_downloads(&dir, 2).unwrap(),
        [dir.join("new.VPK"), dir.join("old.zip")]
    );
    assert_eq!(recent_downloads(&dir, 1).unwrap(), [dir.join("new.VPK")]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn reports_zero_byte_archive_before_attempting_to_unpack() {
    let dir = scratch();
    let path = dir.join("Lifestealer.zip");
    fs::write(&path, []).unwrap();
    let err = extract_vpks(&path, &dir.join("cache")).unwrap_err();
    assert!(
        err.contains("Lifestealer.zip") && err.contains("0 Б"),
        "{err}"
    );
    assert!(!dir.join("cache").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn validates_download_contents_before_accepting_the_file() {
    let dir = scratch();
    let temp = dir.join("download.part");
    fs::write(&temp, b"<html>page instead of mod</html>").unwrap();
    assert!(validate_package(&temp).is_err());
    fs::write(&temp, VPK).unwrap();
    assert!(validate_package(&temp).is_ok());
    for name in ["readme.txt", "looks-like-a-mod.vpk"] {
        let archive = archive(&dir, &[(name, b"not a mod")]);
        assert!(validate_package(&archive).is_err());
    }
    let good = archive(&dir, &[("skin/pak20_dir.vpk", VPK)]);
    fs::copy(good, &temp).unwrap();
    assert!(validate_package(&temp).is_ok());
    fs::remove_dir_all(dir).unwrap();
}
