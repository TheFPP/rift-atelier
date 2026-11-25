use image::{ImageReader, Limits, RgbaImage, imageops};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

type R<T> = Result<T, String>;

fn disk(error: io::Error) -> String {
    format!("Ошибка доступа к файлу: {error}")
}

fn u16le(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn u32le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn term<'a>(tree: &mut &'a [u8]) -> R<&'a str> {
    let end = tree
        .iter()
        .position(|&b| b == 0)
        .ok_or("Повреждена строка индекса VPK.")?;
    let name = std::str::from_utf8(&tree[..end]).map_err(|_| "Некорректное имя в индексе VPK.")?;
    *tree = &tree[end + 1..];
    Ok(name)
}

/// Search only the directory tree. No package data, hashes or signatures are loaded.
pub fn find_targets(
    dota_root: &Path,
    extension: &str,
    query: &str,
    limit: usize,
) -> R<Vec<String>> {
    let want = extension.trim_start_matches('.').to_ascii_lowercase();
    let query = query.trim().replace('\\', "/").to_ascii_lowercase();
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let (data, _) = game_index(dota_root)?;
    let mut tree = data.as_slice();
    let mut found = Vec::new();
    loop {
        let ext = term(&mut tree)?;
        if ext.is_empty() {
            break;
        }
        loop {
            let folder = term(&mut tree)?;
            if folder.is_empty() {
                break;
            }
            loop {
                let name = term(&mut tree)?;
                if name.is_empty() {
                    break;
                }
                let entry = tree.get(..18).ok_or("Неполная запись в индексе VPK.")?;
                let skip = 18 + u16le(&entry[4..]) as usize;
                if u16le(&entry[16..]) != 0xffff {
                    return Err("Повреждена запись индекса VPK.".into());
                }
                tree = tree.get(skip..).ok_or("Оборванная запись в индексе VPK.")?;
                if !ext.eq_ignore_ascii_case(&want) {
                    continue;
                }
                let path = if folder == " " {
                    format!("{name}.{ext}")
                } else {
                    format!("{folder}/{name}.{ext}")
                };
                let lowercase = path.to_ascii_lowercase();
                if tokens.iter().all(|word| lowercase.contains(word)) {
                    found.push(path);
                    if found.len() >= limit {
                        return Ok(found);
                    }
                }
            }
        }
    }
    Ok(found)
}

fn game_index(dota_root: &Path) -> R<(Vec<u8>, usize)> {
    let mut file = File::open(dota_root.join("game/dota/pak01_dir.vpk"))
        .map_err(|_| "Индекс Dota 2 не найден. Выберите папку dota 2 beta.".to_string())?;
    let mut header = [0u8; 12];
    file.read_exact(&mut header).map_err(disk)?;
    let version = u32le(&header[4..]);
    let size = u32le(&header[8..]);
    if u32le(&header) != 0x55aa1234 || !matches!(version, 1 | 2) || size > 128 * 1024 * 1024 {
        return Err("Неизвестный формат индекса VPK.".into());
    }
    if version == 2 {
        file.seek(SeekFrom::Start(28)).map_err(disk)?;
    }
    let mut data = vec![0u8; size as usize];
    file.read_exact(&mut data).map_err(disk)?;
    Ok((data, if version == 2 { 28 } else { 12 }))
}

pub fn bkb_wave(root: &Path, volume: u8) -> Option<Vec<u8>> {
    let (data, header) = game_index(root).ok()?;
    let mut tree = data.as_slice();
    loop {
        let ext = term(&mut tree).ok()?;
        if ext.is_empty() {
            return None;
        }
        loop {
            let folder = term(&mut tree).ok()?;
            if folder.is_empty() {
                break;
            }
            loop {
                let name = term(&mut tree).ok()?;
                if name.is_empty() {
                    break;
                }
                let entry = tree.get(..18)?;
                let preload = u16le(&entry[4..]) as usize;
                let prefix = tree.get(18..18 + preload)?;
                tree = tree.get(18 + preload..)?;
                if (ext, folder, name) != ("vsnd_c", "sounds/items", "black_king_bar") {
                    continue;
                }
                let slot = u16le(&entry[6..]);
                let mut offset = u32le(&entry[8..]) as u64;
                let length = u32le(&entry[12..]) as usize;
                if length + preload > 8 * 1024 * 1024 {
                    return None;
                }
                let archive = if slot == 0x7fff {
                    offset += (data.len() + header) as u64;
                    "pak01_dir.vpk".into()
                } else {
                    format!("pak01_{slot:03}.vpk")
                };
                let mut file = File::open(root.join("game/dota").join(archive)).ok()?;
                file.seek(SeekFrom::Start(offset)).ok()?;
                let mut sound = prefix.to_vec();
                sound.resize(preload + length, 0);
                file.read_exact(&mut sound[preload..]).ok()?;
                if crc32fast::hash(&sound) != u32le(entry) {
                    return None;
                }
                return bkb_pcm(&sound, volume);
            }
        }
    }
}

fn bkb_pcm(sound: &[u8], volume: u8) -> Option<Vec<u8>> {
    let gain = i32::from(volume.min(100));
    let header = sound.get(..16)?;
    let blocks = sound.get(..u32le(header) as usize)?;
    if u16le(&header[6..]) != 1 {
        return None;
    }
    let table = 8 + u32le(&header[8..]) as usize;
    for i in 0..u32le(&header[12..]) as usize {
        let pos = table + 12 * i;
        let block = blocks.get(pos..pos + 12)?;
        if &block[..4] != b"DATA" {
            continue;
        }
        let at = pos + 4 + u32le(&block[4..]) as usize;
        let info = blocks.get(at..at + 32)?;
        let flags = u32le(info);
        let rate = flags >> 14;
        let channels = ((flags >> 7) & 3) as u16;
        let bits = ((flags >> 2) & 31) as u16;
        if flags & 3 != 1 || !matches!(channels, 1 | 2) || !matches!(bits, 8 | 16) || rate == 0 {
            return None;
        }
        let len = u32le(&info[28..]);
        let pcm = sound.get(blocks.len()..)?.get(..len as usize)?;
        let align = channels * bits / 8;
        let mut wave = b"RIFF".to_vec();
        wave.extend((len + 36).to_le_bytes());
        wave.extend(b"WAVEfmt ");
        wave.extend(16u32.to_le_bytes());
        wave.extend(1u16.to_le_bytes());
        wave.extend(channels.to_le_bytes());
        wave.extend(rate.to_le_bytes());
        wave.extend((rate * u32::from(align)).to_le_bytes());
        wave.extend(align.to_le_bytes());
        wave.extend(bits.to_le_bytes());
        wave.extend(b"data");
        wave.extend(len.to_le_bytes());
        if len % u32::from(align) != 0 {
            return None;
        }
        if bits == 8 {
            wave.extend(
                pcm.iter()
                    .map(|&v| (128 + (i32::from(v) - 128) * gain / 100) as u8),
            );
        } else {
            for sample_bytes in pcm.chunks_exact(2) {
                let sample = i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]);
                wave.extend(((i32::from(sample) * gain / 100) as i16).to_le_bytes());
            }
        }
        return Some(wave);
    }
    None
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Write a Source 2 v12 resource with a single aligned DATA block.
/// For a sound or texture, the bytes following FileSize are streamed at runtime.
fn data_block(out: &mut impl Write, resource_version: u16, data: &[u8]) -> R<()> {
    let mut header = [0u8; 32];
    header[..4].copy_from_slice(&(32 + data.len() as u32).to_le_bytes());
    header[4..6].copy_from_slice(&12u16.to_le_bytes());
    header[6..8].copy_from_slice(&resource_version.to_le_bytes());
    header[8..12].copy_from_slice(&8u32.to_le_bytes());
    header[12..16].copy_from_slice(&1u32.to_le_bytes());
    header[16..20].copy_from_slice(b"DATA");
    header[20..24].copy_from_slice(&12u32.to_le_bytes());
    header[24..28].copy_from_slice(&(data.len() as u32).to_le_bytes());
    out.write_all(&header)
        .and_then(|_| out.write_all(data))
        .map_err(disk)
}

fn write_texture(source: &Path, out: &mut impl Write) -> R<()> {
    let mut reader = ImageReader::open(source).map_err(disk)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader
        .decode()
        .map_err(|e| format!("Изображение не прочитано (максимум 4096 × 4096): {e}"))?
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    let levels = width.max(height).ilog2() + 1;
    let mut data = [0u8; 40];
    data[..2].copy_from_slice(&1u16.to_le_bytes());
    data[16..20].copy_from_slice(&1f32.to_le_bytes());
    data[20..22].copy_from_slice(&(width as u16).to_le_bytes());
    data[22..24].copy_from_slice(&(height as u16).to_le_bytes());
    data[24..26].copy_from_slice(&1u16.to_le_bytes());
    data[26] = 4; // VTEX_FORMAT_RGBA8888
    data[27] = levels as u8;
    data[28..32].copy_from_slice(&width.max(height).to_le_bytes());
    data_block(out, 1, &data)?;
    fn emit_mips(base: &RgbaImage, out: &mut impl Write) -> R<()> {
        let (w, h) = base.dimensions();
        if w > 1 || h > 1 {
            let next = imageops::resize(
                base,
                (w / 2).max(1),
                (h / 2).max(1),
                imageops::FilterType::Triangle,
            );
            emit_mips(&next, out)?;
        }
        out.write_all(base.as_raw()).map_err(disk)
    }
    emit_mips(&rgba, out)
}

fn write_wave(source: &Path, out: &mut impl Write) -> R<()> {
    let mut file = File::open(source).map_err(disk)?;
    let file_len = file.metadata().map_err(disk)?.len();
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff).map_err(disk)?;
    let riff_end = 8 + u32le(&riff[4..]) as u64;
    if &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" || riff_end > file_len {
        return Err("Некорректный WAV-файл.".into());
    }
    let mut format = None;
    let mut samples = None;
    let mut pos = 12u64;
    while pos + 8 <= riff_end {
        file.seek(SeekFrom::Start(pos)).map_err(disk)?;
        let mut chunk = [0u8; 8];
        file.read_exact(&mut chunk).map_err(disk)?;
        let size = u32le(&chunk[4..]);
        let start = pos + 8;
        let end = start + size as u64;
        if end > riff_end {
            return Err("Оборван блок WAV.".into());
        }
        match &chunk[..4] {
            b"fmt " if size >= 16 => {
                let mut data = [0u8; 16];
                file.read_exact(&mut data).map_err(disk)?;
                format = Some(data);
            }
            b"data" if samples.is_none() => samples = Some((start, size)),
            _ => {}
        }
        pos = end + (size % 2) as u64;
    }
    let fmt = format.ok_or("В WAV отсутствует описание формата.")?;
    let (start, length) = samples.ok_or("В WAV отсутствуют звуковые данные.")?;
    let channels = u16le(&fmt[2..]) as u32;
    let rate = u32le(&fmt[4..]);
    let align = u16le(&fmt[12..]) as u32;
    let bits = u16le(&fmt[14..]) as u32;
    if u16le(&fmt) != 1
        || !matches!(channels, 1 | 2)
        || !matches!(bits, 8 | 16)
        || !(1..=65535).contains(&rate)
        || align != channels * bits / 8
        || length == 0
        || length % align != 0
    {
        return Err(
            "Сохраните WAV как PCM 8 или 16 бит, моно или стерео, с частотой до 65 535 Гц.".into(),
        );
    }
    // Dota's v1 sound header stores rate, format and frame size in a bit field.
    let flags = (rate << 14) | (1 << 12) | (align << 9) | (channels << 7) | (bits << 2) | 1;
    let frames = length / align;
    let mut data = [0u8; 40];
    data[..4].copy_from_slice(&flags.to_le_bytes());
    data[4..8].copy_from_slice(&(-1i32).to_le_bytes());
    data[8..12].copy_from_slice(&frames.to_le_bytes());
    data[12..16].copy_from_slice(&(frames as f32 / rate as f32).to_le_bytes());
    data[28..32].copy_from_slice(&length.to_le_bytes());
    data_block(out, 1, &data)?;
    file.seek(SeekFrom::Start(start)).map_err(disk)?;
    let copied = io::copy(&mut file.take(length as u64), out).map_err(disk)?;
    if copied != length as u64 {
        return Err("Звуковой файл изменился во время чтения.".into());
    }
    Ok(())
}

fn write_compiled(source: &Path, out: &mut impl Write) -> R<()> {
    let mut file = File::open(source).map_err(disk)?;
    let mut header = [0u8; 16];
    file.read_exact(&mut header).map_err(disk)?;
    let size = u32le(&header) as u64;
    let blocks = u32le(&header[12..]);
    let table = 8 + u32le(&header[8..]) as u64;
    if u16le(&header[4..]) != 12
        || blocks == 0
        || blocks > 1024
        || table + blocks as u64 * 12 > size
        || size > file.metadata().map_err(disk)?.len()
    {
        return Err("Неверный заголовок скомпилированного ресурса Source 2.".into());
    }
    file.seek(SeekFrom::Start(table)).map_err(disk)?;
    let mut has_data = false;
    for n in 0..blocks {
        let mut entry = [0u8; 12];
        file.read_exact(&mut entry).map_err(disk)?;
        let start = table + 12 * n as u64 + 4 + u32le(&entry[4..]) as u64;
        if start + u32le(&entry[8..]) as u64 > size {
            return Err("Повреждена таблица блоков Source 2.".into());
        }
        has_data |= &entry[..4] == b"DATA";
    }
    if !has_data {
        return Err("В ресурсе Source 2 отсутствует блок DATA.".into());
    }
    file.rewind().map_err(disk)?;
    io::copy(&mut file, out).map_err(disk)?;
    Ok(())
}

fn compile_model(dota_root: &Path, model: &Path) -> R<PathBuf> {
    let compiler = dota_root.join("game/bin/win64/resourcecompiler.exe");
    if !compiler.is_file() {
        return Err("Для .vmdl включите Dota 2 Workshop Tools в Steam → Dota 2 → Свойства → Доп. контент. Также можно открыть готовую .vmdl_c.".into());
    }
    let src = model.canonicalize().map_err(disk)?;
    let root = dota_root
        .join("content/dota_addons")
        .canonicalize()
        .map_err(disk)?;
    let rel = src.strip_prefix(&root).map_err(
        |_| "Сохраните модель и её зависимости в content/dota_addons/<аддон>/ через ModelDoc.",
    )?;
    if rel.components().count() < 3 {
        return Err("Выберите .vmdl из каталога models проекта Workshop Tools.".into());
    }
    let mut child = Command::new(compiler);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        child.creation_flags(0x08000000);
    }
    // Pass the authored path to preserve the project's FBX / material references.
    let result = child
        .arg("-game")
        .arg(dota_root.join("game/dota"))
        .arg("-i")
        .arg(model)
        .output()
        .map_err(disk)?;
    let compiled = dota_root
        .join("game/dota_addons")
        .join(rel)
        .with_extension("vmdl_c");
    if !result.status.success() || !compiled.is_file() {
        let report = format!(
            "{} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let tail: String = report
            .chars()
            .rev()
            .take(900)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        return Err(format!(
            "Workshop Tools не удалось собрать модель. Проверьте ссылки на FBX и материалы. {}",
            tail.trim()
        ));
    }
    Ok(compiled)
}

/// Convert to a single compiled target. Additional model resources retain their original
/// Workshop paths and must also be present in the destination add-on.
pub fn compile_source(dota_root: &Path, source: &Path, target: &str, output: &Path) -> R<()> {
    let src_type = extension(source);
    let dst_type = extension(Path::new(target));
    let valid = match src_type.as_str() {
        "jpg" | "jpeg" | "png" | "tga" => dst_type == "vtex_c",
        "wav" => dst_type == "vsnd_c",
        "vmdl" => dst_type == "vmdl_c",
        "vmdl_c" | "vtex_c" | "vsnd_c" | "vmat_c" => src_type == dst_type,
        "fbx" => {
            return Err(
                "Откройте FBX в ModelDoc и сохраните .vmdl с привязками материалов и скелета."
                    .into(),
            );
        }
        _ => false,
    };
    if !valid {
        return Err("Исходный формат не соответствует выбранному типу ресурса.".into());
    }
    let parent = output.parent().ok_or("Не выбрана папка назначения.")?;
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent).map_err(disk)?;
    }
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let (temp, file) = loop {
        let name = format!(
            ".rift-resource-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => break (path, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(disk(e)),
        }
    };
    let result = (|| {
        let mut out = BufWriter::new(file);
        match src_type.as_str() {
            "jpg" | "jpeg" | "png" | "tga" => write_texture(source, &mut out)?,
            "wav" => write_wave(source, &mut out)?,
            "vmdl" => write_compiled(&compile_model(dota_root, source)?, &mut out)?,
            _ => write_compiled(source, &mut out)?,
        }
        out.flush().map_err(disk)?;
        drop(out);
        fs::rename(&temp, output).map_err(disk)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
