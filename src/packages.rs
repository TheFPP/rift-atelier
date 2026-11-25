use crc32fast::Hasher;
use std::{
    collections::BTreeMap,
    fmt::Display,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAGIC: u32 = 0x55aa_1234;
const INLINE: u16 = 0x7fff;
const TREE_LIMIT: usize = 32 * 1024 * 1024;
type Result<T, E = String> = std::result::Result<T, E>;

#[derive(Clone, Debug)]
pub enum Input {
    Vpk(PathBuf),
    Loose { source: PathBuf, target: String },
}

fn err(e: impl Display) -> String {
    e.to_string()
}
fn invalid() -> String {
    "Повреждён каталог VPK.".into()
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    ext: String,
    dir: String,
    name: String,
}

impl Key {
    fn parse(path: &str) -> Result<Self> {
        if path.len() > 2048
            || path
                .chars()
                .any(|c| c.is_control() || ":*?\"<>|".contains(c))
        {
            return Err(format!("Недопустимый путь ресурса: {path}"));
        }
        let path = path.replace('\\', "/").to_ascii_lowercase();
        if path
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == ".." || c.ends_with(['.', ' ']))
        {
            return Err(format!("Недопустимый путь ресурса: {path}"));
        }
        let (dir, filename) = path.rsplit_once('/').unwrap_or(("", &path));
        let (name, ext) = filename.rsplit_once('.').unwrap_or((filename, ""));
        if name.is_empty()
            || matches!(
                ext,
                "exe"
                    | "dll"
                    | "bat"
                    | "cmd"
                    | "ps1"
                    | "vbs"
                    | "lua"
                    | "nut"
                    | "cfg"
                    | "vcfg"
                    | "gi"
                    | "signatures"
            )
            || path.starts_with("cfg/")
            || path.starts_with("scripts/vscripts/")
            || path == "scripts/items/items_game.txt"
        {
            return Err(format!(
                "В пакете есть файл, который нельзя применить как визуальную или звуковую замену: {path}"
            ));
        }
        Ok(Self {
            ext: ext.into(),
            dir: dir.into(),
            name: name.into(),
        })
    }
    fn path(&self) -> String {
        let mut s = String::new();
        if !self.dir.is_empty() {
            s.push_str(&self.dir);
            s.push('/');
        }
        s.push_str(&self.name);
        if !self.ext.is_empty() {
            s.push('.');
            s.push_str(&self.ext);
        }
        s
    }
}

#[derive(Clone, Copy)]
struct Span {
    offset: u64,
    len: u32,
}
struct Member {
    source: usize,
    spans: [Span; 2],
    expected: Option<u32>,
}
impl Member {
    fn len(&self) -> Result<u32> {
        self.spans[0]
            .len
            .checked_add(self.spans[1].len)
            .ok_or_else(|| "Ресурс превышает 4 ГБ.".into())
    }
    fn stream(
        &self,
        reader: &mut File,
        buffer: &mut [u8],
        mut write: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<u32> {
        let mut crc = Hasher::new();
        for span in &self.spans {
            if span.len == 0 {
                continue;
            }
            reader.seek(SeekFrom::Start(span.offset)).map_err(err)?;
            let mut left = span.len as usize;
            while left > 0 {
                let size = left.min(buffer.len());
                let bytes = &mut buffer[..size];
                reader.read_exact(bytes).map_err(err)?;
                crc.update(bytes);
                write(bytes)?;
                left -= size;
            }
        }
        Ok(crc.finalize())
    }
}

struct Tree<'a> {
    buf: &'a [u8],
    cursor: usize,
}
impl<'a> Tree<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.cursor.checked_add(n).ok_or_else(invalid)?;
        let data = self.buf.get(self.cursor..end).ok_or_else(invalid)?;
        self.cursor = end;
        Ok(data)
    }
    fn word(&mut self) -> Result<&'a str> {
        let len = self.buf[self.cursor..]
            .iter()
            .take(2049)
            .position(|&b| b == 0)
            .ok_or_else(invalid)?;
        let value = self.take(len + 1)?;
        std::str::from_utf8(&value[..len]).map_err(|_| invalid())
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

fn index(
    file: &mut File,
    source: usize,
    mut visit: impl FnMut(Key, Member, &mut File) -> Result<()>,
) -> Result<()> {
    let size = file.metadata().map_err(err)?.len();
    let mut header = [0; 28];
    file.read_exact(&mut header[..12]).map_err(|_| invalid())?;
    let mut h = Tree {
        buf: &header[..12],
        cursor: 0,
    };
    if h.u32()? != MAGIC {
        return Err("Файл не является архивом Valve VPK.".into());
    }
    let version = h.u32()?;
    let tree_size = h.u32()? as usize;
    let (header_size, data_size) = match version {
        1 => (12u64, size.saturating_sub(12 + tree_size as u64)),
        2 => {
            file.read_exact(&mut header[12..]).map_err(|_| invalid())?;
            let mut sections = Tree {
                buf: &header[12..],
                cursor: 0,
            };
            let data = sections.u32()? as u64;
            let trailing = sections.u32()? as u64 + sections.u32()? as u64 + sections.u32()? as u64;
            if 28 + tree_size as u64 + data + trailing > size {
                return Err(invalid());
            }
            (28, data)
        }
        _ => return Err(format!("Версия VPK {version} не поддерживается.")),
    };
    let base = header_size + tree_size as u64;
    if tree_size > TREE_LIMIT || tree_size == 0 || base > size {
        return Err(invalid());
    }
    let mut bytes = vec![0; tree_size];
    file.read_exact(&mut bytes).map_err(err)?;
    let mut tree = Tree {
        buf: &bytes,
        cursor: 0,
    };
    let mut path_bytes = 0;
    let mut count = 0;
    loop {
        let ext = tree.word()?;
        if ext.is_empty() {
            break;
        }
        if ext.contains(['.', '/', '\\']) {
            return Err(invalid());
        }
        loop {
            let dir = tree.word()?;
            if dir.is_empty() {
                break;
            }
            loop {
                let name = tree.word()?;
                if name.is_empty() {
                    break;
                }
                if name.contains(['/', '\\']) {
                    return Err(invalid());
                }
                let dir = if dir == " " { "" } else { dir };
                let ext = if ext == " " { "" } else { ext };
                let path = Key {
                    dir: dir.into(),
                    name: name.into(),
                    ext: ext.into(),
                }
                .path();
                count += 1;
                path_bytes += path.len();
                if count > 180_000 || path_bytes > TREE_LIMIT {
                    return Err("В моде слишком много ресурсов.".into());
                }
                let key = Key::parse(&path)?;
                let crc = tree.u32()?;
                let preload = tree.u16()? as u32;
                let archive = tree.u16()?;
                let offset = tree.u32()? as u64;
                let len = tree.u32()?;
                if tree.u16()? != 0xffff {
                    return Err(invalid());
                }
                if len > 0 && archive != INLINE {
                    return Err("Это многотомный VPK. Для импорта нужен самодостаточный VPK с данными внутри.".into());
                }
                if len > 0 && offset + len as u64 > data_size {
                    return Err(invalid());
                }
                let before = header_size + tree.cursor as u64;
                tree.take(preload as usize)?;
                let member = Member {
                    source,
                    spans: [
                        Span {
                            offset: before,
                            len: preload,
                        },
                        Span {
                            offset: base + offset,
                            len,
                        },
                    ],
                    expected: Some(crc),
                };
                visit(key, member, file)?;
            }
        }
    }
    if tree.cursor != bytes.len() || count == 0 {
        return Err(invalid());
    }
    Ok(())
}

pub fn inspect(path: &Path) -> Result<Vec<String>> {
    let mut file = File::open(path).map_err(err)?;
    let mut names = Vec::new();
    index(&mut file, 0, |key, _, _| {
        names.push(key.path());
        Ok(())
    })?;
    Ok(names)
}

/// Verify each resource before accepting a package into the library.
pub fn verify_import(path: &Path) -> Result<usize> {
    let mut file = File::open(path).map_err(err)?;
    let mut buffer = [0; 64 * 1024];
    let mut count = 0;
    let mut total = 0u32;
    index(&mut file, 0, |key, member, reader| {
        // The data section in an installable VPK has a 32-bit length.
        total = total
            .checked_add(member.len()?)
            .ok_or("Ресурсы мода превышают предел размера VPK в 4 ГиБ.")?;
        let actual = member.stream(reader, &mut buffer, |_| Ok(()))?;
        if member.expected != Some(actual) {
            return Err(format!(
                "Повреждён ресурс «{}». Контрольная сумма не совпадает. Скачайте мод повторно.",
                key.path()
            ));
        }
        count += 1;
        Ok(())
    })?;
    Ok(count)
}

/// Read a single verified asset without extracting or mapping the whole VPK.
pub fn read_member(path: &Path, target: &str, limit: usize) -> Result<Vec<u8>> {
    let mut file = File::open(path).map_err(err)?;
    let wanted = Key::parse(target)?;
    let mut selected = None;
    index(&mut file, 0, |key, member, _| {
        if key == wanted {
            selected = Some(member);
        }
        Ok(())
    })?;
    let member = selected.ok_or("Ресурс отсутствует в VPK.")?;
    let len = member.len()? as usize;
    if len > limit {
        return Err("Изображение в VPK слишком велико для предпросмотра.".into());
    }
    let mut bytes = vec![0u8; len];
    let mut pos = 0;
    for span in member.spans {
        if span.len != 0 {
            let end = pos + span.len as usize;
            file.seek(SeekFrom::Start(span.offset)).map_err(err)?;
            file.read_exact(&mut bytes[pos..end]).map_err(err)?;
            pos = end;
        }
    }
    if member.expected != Some(crc32fast::hash(&bytes)) {
        return Err("Контрольная сумма ресурса в VPK не совпадает.".into());
    }
    Ok(bytes)
}

struct Scratch(PathBuf);
impl Scratch {
    fn create(dir: &Path) -> Result<(Self, File)> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let name = format!(
                ".rift-atelier-{}-{}.tmp",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            );
            let path = dir.join(name);
            match File::create_new(&path) {
                Ok(file) => return Ok((Self(path), file)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(err(e)),
            }
        }
        Err("Не удалось создать временный файл.".into())
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn cstring(dst: &mut Vec<u8>, s: &str) {
    dst.extend(s.as_bytes());
    dst.push(0);
}
fn grouped_word(dst: &mut Vec<u8>, s: &str) {
    cstring(dst, if s.is_empty() { " " } else { s })
}

fn build(inputs: &[Input], mut output: File) -> Result<usize> {
    let mut sources = Vec::new();
    let mut members = BTreeMap::new();
    for input in inputs {
        let path = match input {
            Input::Vpk(p) => p,
            Input::Loose { source, .. } => source,
        };
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let id = sources.len();
        match input {
            Input::Vpk(_) => index(&mut file, id, |key, data, _| {
                members.insert(key, data);
                Ok(())
            })?,
            Input::Loose { target, .. } => {
                let key = Key::parse(target)?;
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !key.ext.ends_with("_c") || !ext.eq_ignore_ascii_case(&key.ext) {
                    return Err(format!(
                        "Тип исходного файла и цель должны совпадать. Нужен готовый ресурс .{}.",
                        key.ext
                    ));
                }
                let len = u32::try_from(file.metadata().map_err(err)?.len())
                    .map_err(|_| "Ресурс превышает 4 ГБ.")?;
                members.insert(
                    key,
                    Member {
                        source: id,
                        spans: [Span { offset: 0, len: 0 }, Span { offset: 0, len }],
                        expected: None,
                    },
                );
            }
        }
        sources.push(file);
    }
    if members.is_empty() {
        return Err("Добавьте хотя бы одну замену в профиль.".into());
    }
    let mut tree = Vec::new();
    let mut checksums = Vec::with_capacity(members.len());
    let mut chunk = [0; 64 * 1024];
    let mut prev: Option<&Key> = None;
    let mut data_bytes = 0u32;
    for (key, m) in &members {
        let new_ext = prev.is_none_or(|p| p.ext != key.ext);
        let new_dir = new_ext || prev.is_none_or(|p| p.dir != key.dir);
        if new_dir && prev.is_some() {
            tree.push(0)
        }
        if new_ext && prev.is_some() {
            tree.push(0)
        }
        if new_ext {
            grouped_word(&mut tree, &key.ext)
        }
        if new_dir {
            grouped_word(&mut tree, &key.dir)
        }
        cstring(&mut tree, &key.name);
        // Existing VPKs declare their CRC in the directory; only loose files need a first pass.
        let checksum = match m.expected {
            Some(crc) => crc,
            None => m.stream(&mut sources[m.source], &mut chunk, |_| Ok(()))?,
        };
        checksums.push(checksum);
        tree.extend(checksum.to_le_bytes());
        tree.extend(0u16.to_le_bytes());
        tree.extend(INLINE.to_le_bytes());
        tree.extend(data_bytes.to_le_bytes());
        tree.extend(m.len()?.to_le_bytes());
        tree.extend([0xff, 0xff]);
        data_bytes = data_bytes
            .checked_add(m.len()?)
            .ok_or("Пакет превышает предел VPK в 4 ГБ.")?;
        prev = Some(key);
    }
    tree.extend([0; 3]);
    if tree.len() > TREE_LIMIT {
        return Err("Индекс профиля слишком велик.".into());
    }
    let mut whole = md5::Context::new();
    let mut write_hashed = |bytes: &[u8]| -> Result<()> {
        output.write_all(bytes).map_err(err)?;
        whole.consume(bytes);
        Ok(())
    };
    for value in [MAGIC, 2, tree.len() as u32, data_bytes, 0, 48, 0] {
        write_hashed(&value.to_le_bytes())?;
    }
    write_hashed(&tree)?;
    for ((key, m), expected) in members.iter().zip(checksums) {
        let checksum = m.stream(&mut sources[m.source], &mut chunk, &mut write_hashed)?;
        if expected != checksum {
            return Err(format!("Ошибка контрольной суммы VPK: {}", key.path()));
        }
    }
    // VPK v2: tree MD5, empty archive-hash section MD5, then MD5 of everything so far.
    write_hashed(&md5::compute(&tree).0)?;
    write_hashed(&md5::compute([]).0)?;
    output.write_all(&whole.finalize().0).map_err(err)?;
    output.sync_all().map_err(err)?;
    Ok(members.len())
}

// MoveFileW fails if a destination exists, including when another process won
// a race for a new slot. The temporary file is on the same filesystem.
#[cfg(windows)]
fn publish_new(source: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileW(existing: *const u16, new: *const u16) -> i32;
    }
    let src: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let dst: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileW(src.as_ptr(), dst.as_ptr()) } == 0 {
        return Err(err(io::Error::last_os_error()));
    }
    Ok(())
}
#[cfg(not(windows))]
fn publish_new(source: &Path, target: &Path) -> Result<()> {
    fs::hard_link(source, target).map_err(err)
}

pub fn export(inputs: &[Input], output: &Path) -> Result<usize> {
    if output.try_exists().map_err(err)? {
        return Err("Этот файл уже существует. Выберите новое имя для экспорта.".into());
    }
    let dir = output.parent().ok_or("Выберите папку для экспорта.")?;
    let (scratch, file) = Scratch::create(dir)?;
    let count = build(inputs, file)?;
    publish_new(&scratch.0, output)?;
    Ok(count)
}

const STATE: &str = ".rift-atelier.install";
const LOCK: &str = ".rift-atelier.lock";

fn checked_metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            #[cfg(windows)]
            let redirected = {
                use std::os::windows::fs::MetadataExt;
                meta.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let redirected = meta.file_type().is_symlink();
            if redirected {
                return Err(format!(
                    "Путь содержит ссылку или точку подключения: {}",
                    path.display()
                ));
            }
            Ok(Some(meta))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(err(e)),
    }
}

fn language_dir(root: &Path, language: &str) -> Result<PathBuf> {
    if !matches!(language, "russian" | "koreana" | "schinese") {
        return Err("Выберите русский, корейский или китайский язык озвучки Dota 2.".into());
    }
    let game = root.join("game");
    checked_metadata(&game)?;
    let dir = game.join(format!("dota_{language}"));
    checked_metadata(&dir)?;
    Ok(dir)
}

/// A startup check reads the small journal without hashing or locking the mod.
pub fn receipt_present(root: &Path, language: &str) -> Result<bool> {
    Ok(Receipt::load(&language_dir(root, language)?)?.is_some())
}

/// Fast startup check. Full byte validation is available through verified_installation.
pub fn receipt_has_payload(root: &Path, language: &str) -> Result<bool> {
    let dir = language_dir(root, language)?;
    let Some(receipt) = Receipt::load(&dir)? else {
        return Ok(false);
    };
    Ok(checked_metadata(&slot_path(&dir, receipt.slot))?
        .is_some_and(|file| file.is_file() && file.len() == receipt.staged.len))
}

/// Confirm the published file is the staged archive, including its payload bytes.
pub fn verified_installation(root: &Path, language: &str) -> Result<(PathBuf, usize)> {
    let dir = language_dir(root, language)?;
    let receipt = Receipt::load(&dir)?.ok_or("Для этого языка нет установленного профиля.")?;
    let path = slot_path(&dir, receipt.slot);
    if Stamp::read(&path)? != Some(receipt.staged) {
        return Err(format!(
            "Файл {} отсутствует или отличается от последней сборки.",
            path.display()
        ));
    }
    let count = inspect(&path)?.len();
    Ok((path, count))
}

fn mounted(root: &Path, language: &str) -> Result<(PathBuf, File)> {
    let dir = language_dir(root, language)?;
    if !root.join("game/dota/gameinfo.gi").is_file() {
        return Err("Укажите папку Dota 2 с файлом game/dota/gameinfo.gi.".into());
    }
    if !dir.is_dir() {
        return Err(format!(
            "Папка {} отсутствует. Проверьте выбранный язык и локальные файлы Dota 2.",
            dir.display()
        ));
    }
    let path = dir.join(LOCK);
    checked_metadata(&path)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(err)?;
    lock.try_lock()
        .map_err(|_| "С этой папкой уже работает другой экземпляр приложения.".to_string())?;
    Ok((dir, lock))
}

// A receipt stores the previous and staged fingerprints before an atomic
// replace. Both sides of a power interruption are therefore recognisable.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    crc: u32,
}
impl Stamp {
    fn read(path: &Path) -> Result<Option<Self>> {
        let Some(m) = checked_metadata(path)? else {
            return Ok(None);
        };
        if !m.is_file() {
            return Err(format!("Ожидался обычный файл: {}", path.display()));
        }
        let mut crc = Hasher::new();
        let mut file = File::open(path).map_err(err)?;
        let mut buf = [0; 64 * 1024];
        let mut len = 0;
        loop {
            let n = file.read(&mut buf).map_err(err)?;
            if n == 0 {
                break;
            }
            crc.update(&buf[..n]);
            len += n as u64;
        }
        Ok(Some(Self {
            len,
            crc: crc.finalize(),
        }))
    }
    fn encode(self) -> String {
        format!("{}-{:08x}", self.len, self.crc)
    }
    fn parse(s: &str) -> Option<Self> {
        let (len, crc) = s.split_once('-')?;
        Some(Self {
            len: len.parse().ok()?,
            crc: u32::from_str_radix(crc, 16).ok()?,
        })
    }
}

struct Receipt {
    slot: u8,
    previous: Option<Stamp>,
    staged: Stamp,
}
impl Receipt {
    fn load(dir: &Path) -> Result<Option<Self>> {
        let path = dir.join(STATE);
        let Some(m) = checked_metadata(&path)? else {
            return Ok(None);
        };
        let broken = "Запись об установленном профиле повреждена. Проверьте файл .rift-atelier.install в папке языка.";
        if !m.is_file() || m.len() > 256 {
            return Err(broken.into());
        }
        let raw = fs::read_to_string(path).map_err(err)?;
        let words: Vec<_> = raw.split_whitespace().collect();
        let ["rift-atelier/1", s, prev, next] = words.as_slice() else {
            return Err(broken.into());
        };
        let slot = s
            .parse::<u8>()
            .ok()
            .filter(|v| (2..=99).contains(v))
            .ok_or(broken)?;
        let previous = if *prev == "-" {
            None
        } else {
            Some(Stamp::parse(prev).ok_or(broken)?)
        };
        Ok(Some(Self {
            slot,
            previous,
            staged: Stamp::parse(next).ok_or(broken)?,
        }))
    }
    fn save(&self, dir: &Path) -> Result<()> {
        let (scratch, mut file) = Scratch::create(dir)?;
        let previous = self
            .previous
            .map(Stamp::encode)
            .unwrap_or_else(|| "-".into());
        writeln!(
            file,
            "rift-atelier/1 {} {} {}",
            self.slot,
            previous,
            self.staged.encode()
        )
        .map_err(err)?;
        file.sync_all().map_err(err)?;
        drop(file);
        fs::rename(&scratch.0, dir.join(STATE)).map_err(err)?;
        Ok(())
    }
    fn owned_stamp(&self, dir: &Path) -> Result<Option<Stamp>> {
        let path = slot_path(dir, self.slot);
        let stamp = Stamp::read(&path)?;
        if stamp.is_some_and(|v| v != self.staged && Some(v) != self.previous) {
            return Err(format!(
                "Файл {} изменился после установки. Переместите его в другую папку, чтобы продолжить.",
                path.display()
            ));
        }
        Ok(stamp)
    }
}

fn slot_path(dir: &Path, slot: u8) -> PathBuf {
    dir.join(format!("pak{slot:02}_dir.vpk"))
}
fn free_slot(dir: &Path) -> Result<u8> {
    let mut taken = [false; 100];
    for e in fs::read_dir(dir).map_err(err)? {
        let name = e
            .map_err(err)?
            .file_name()
            .to_string_lossy()
            .to_ascii_lowercase();
        if name.starts_with("pak")
            && name.ends_with(".vpk")
            && name.as_bytes().get(5) == Some(&b'_')
            && let Some(slot) = name.get(3..5).and_then(|s| s.parse::<usize>().ok())
        {
            taken[slot] = true
        }
    }
    (2..=99u8)
        .find(|s| !taken[*s as usize] && !(64..=67).contains(s))
        .ok_or_else(|| "Все слоты VPK в папке языка заняты.".into())
}

#[cfg(test)]
pub fn install(inputs: &[Input], dota_root: &Path, language: &str) -> Result<usize> {
    install_verified(inputs, dota_root, language).map(|(_, count)| count)
}

/// Keep the mount lock through readback, including when another copy is running.
pub fn install_verified(
    inputs: &[Input],
    dota_root: &Path,
    language: &str,
) -> Result<(PathBuf, usize)> {
    let (dir, _lock) = mounted(dota_root, language)?;
    let receipt = Receipt::load(&dir)?;
    let (slot, previous) = match &receipt {
        Some(r) => (r.slot, r.owned_stamp(&dir)?),
        None => (free_slot(&dir)?, None),
    };
    let (scratch, file) = Scratch::create(&dir)?;
    let count = build(inputs, file)?;
    let staged = Stamp::read(&scratch.0)?.ok_or("Временный пакет недоступен.")?;
    let target = slot_path(&dir, slot);
    if let Some(r) = &receipt
        && r.owned_stamp(&dir)? != previous
    {
        return Err("Пакет изменился во время сборки. Повторите операцию.".into());
    }
    Receipt {
        slot,
        previous,
        staged,
    }
    .save(&dir)?;
    let result = if previous.is_some() {
        fs::rename(&scratch.0, &target).map_err(err)
    } else {
        publish_new(&scratch.0, &target)
    };
    if result.is_err() && receipt.is_none() {
        fs::remove_file(dir.join(STATE)).map_err(err)?;
    }
    result?;
    let (verified_path, verified_count) = verified_installation(dota_root, language)?;
    if verified_count != count || verified_path != target {
        return Err("Состав установленного пакета отличается от собранного профиля.".into());
    }
    Ok((verified_path, verified_count))
}

pub fn uninstall(dota_root: &Path, language: &str) -> Result<()> {
    let (dir, _lock) = mounted(dota_root, language)?;
    let receipt =
        Receipt::load(&dir)?.ok_or("В этой папке языка пока нет установленного профиля.")?;
    if receipt.owned_stamp(&dir)?.is_some() {
        fs::remove_file(slot_path(&dir, receipt.slot)).map_err(err)?;
    }
    fs::remove_file(dir.join(STATE)).map_err(err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::SystemTime,
    };

    struct Sandbox(PathBuf);
    impl Sandbox {
        fn new() -> Self {
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let stamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "rift-vpk-test-{}-{stamp}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(dir.join("game/dota")).unwrap();
            fs::write(dir.join("game/dota/gameinfo.gi"), b"fixture").unwrap();
            fs::create_dir_all(dir.join("game/dota_russian")).unwrap();
            Self(dir)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
        fn loose(&self, name: &str, value: &[u8], target: &str) -> Input {
            let path = self.path(name);
            fs::write(&path, value).unwrap();
            Input::Loose {
                source: path,
                target: target.into(),
            }
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // Independent byte-level fixture. Embedded bytes plus data form "123456789".
    fn example(version: u32, archive: u16, folder: &str) -> Vec<u8> {
        let mut tree = b"vtex_c\0".to_vec();
        tree.extend(folder.as_bytes());
        tree.extend(b"\0skin\0");
        tree.extend(0xCBF4_3926u32.to_le_bytes());
        tree.extend(3u16.to_le_bytes());
        tree.extend(archive.to_le_bytes());
        tree.extend(0u32.to_le_bytes());
        tree.extend(6u32.to_le_bytes());
        tree.extend(0xffffu16.to_le_bytes());
        tree.extend(b"123\0\0\0");
        let mut out = 0x55AA1234u32.to_le_bytes().to_vec();
        out.extend(version.to_le_bytes());
        out.extend((tree.len() as u32).to_le_bytes());
        if version == 2 {
            out.extend(6u32.to_le_bytes());
            out.extend([0; 12]);
        }
        out.extend(tree);
        out.extend(b"456789");
        out
    }
    #[test]
    fn import_rejects_damaged_member_before_adding_it_to_library() {
        let d = Sandbox::new();
        let library = d.path("library");
        let path = d.path("textures.vpk");
        let mut payload = example(2, INLINE, "materials");
        *payload.last_mut().unwrap() ^= 1;
        fs::write(&path, &payload).unwrap();
        let result = crate::library::import(&library, &d.0, &path, "");
        let detail = result
            .err()
            .expect("damaged payload was added to the library");
        assert!(detail.contains("materials/skin.vtex_c"), "{detail}");
        assert_eq!(fs::read_dir(library.join("assets")).unwrap().count(), 0);
        fs::write(&path, example(2, INLINE, "materials")).unwrap();
        let imported = crate::library::import(&library, &d.0, &path, "").unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].count, 1);
    }

    #[test]
    fn zip_with_one_damaged_variant_rolls_back_the_entire_import() {
        let d = Sandbox::new();
        let library = d.path("library");
        let path = d.path("variants.zip");
        let valid = example(1, INLINE, "materials");
        let mut damaged = valid.clone();
        *damaged.last_mut().unwrap() ^= 1;
        let mut writer = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, bytes) in [("blue.vpk", &valid), ("red.vpk", &damaged)] {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
        let result = crate::library::import(&library, &d.0, &path, "");
        let detail = result.err().expect("a damaged variant was imported");
        assert!(
            detail.contains("red.vpk") && detail.contains("materials/skin.vtex_c"),
            "{detail}"
        );
        assert_eq!(fs::read_dir(library.join("assets")).unwrap().count(), 0);
    }

    #[test]
    fn imports_both_versions_and_preserves_preloaded_bytes() {
        let d = Sandbox::new();
        for v in [1, 2] {
            let src = d.path(&format!("in{v}.vpk"));
            let dst = d.path(&format!("out{v}.vpk"));
            fs::write(&src, example(v, 0x7fff, "materials")).unwrap();
            assert_eq!(inspect(&src).unwrap(), ["materials/skin.vtex_c"]);
            assert_eq!(export(&[Input::Vpk(src)], &dst).unwrap(), 1);
            let raw = fs::read(dst).unwrap();
            assert_eq!(&raw[..8], &[0x34, 0x12, 0xaa, 0x55, 2, 0, 0, 0]);
            let tree = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
            let tail = 28 + tree + 9;
            assert_eq!(&raw[28 + tree..tail], b"123456789");
            assert_eq!(&raw[16..28], &[0, 0, 0, 0, 48, 0, 0, 0, 0, 0, 0, 0]);
            // Reference digests computed independently from this exact one-file layout.
            let hashes = raw[tail..]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            assert_eq!(
                hashes,
                concat!(
                    "24dd84496066856a650955a4cbfca06a",
                    "d41d8cd98f00b204e9800998ecf8427e",
                    "20032aa945a69b491fd390d147deabc7"
                )
            );
        }
    }
    #[test]
    fn last_enabled_input_wins_and_preserves_other_targets() {
        let d = Sandbox::new();
        let a = d.loose("a.vtex_c", b"first", "materials/skin.vtex_c");
        let b = d.loose("b.vtex_c", b"last", "MATERIALS/SKIN.VTEX_C");
        let c = d.loose("c.vsnd_c", b"sound", "sounds/horn.vsnd_c");
        let dst = d.path("merged.vpk");
        assert_eq!(export(&[a, b, c], &dst).unwrap(), 2);
        let mut names = inspect(&dst).unwrap();
        names.sort();
        assert_eq!(names, ["materials/skin.vtex_c", "sounds/horn.vsnd_c"]);
        let raw = fs::read(dst).unwrap();
        assert!(raw.windows(4).any(|x| x == b"last"));
        assert!(!raw.windows(5).any(|x| x == b"first"));
    }
    #[test]
    fn rejects_bad_crc_split_archives_and_unsafe_paths_without_publishing() {
        let d = Sandbox::new();
        let p = d.path("src.vpk");
        let dst = d.path("output.vpk");
        for bad in [
            example(2, 2, "materials"),
            example(1, 0x7fff, "../outside"),
            {
                let mut b = example(2, 0x7fff, "materials");
                *b.last_mut().unwrap() = 0;
                b
            },
            example(2, 0x7fff, "materials")[..40].to_vec(),
        ] {
            fs::write(&p, bad).unwrap();
            assert!(export(&[Input::Vpk(p.clone())], &dst).is_err());
            assert!(!dst.exists());
        }
        let png = d.loose("input.png", b"png signature", "materials/skin.vtex_c");
        assert!(export(&[png], &dst).is_err());
    }
    #[test]
    fn install_switches_and_removes_only_owned_package() {
        let d = Sandbox::new();
        let lang = d.path("game/dota_russian");
        assert!(!receipt_present(&d.0, "russian").unwrap());
        assert_eq!(fs::read_dir(&lang).unwrap().count(), 0);
        fs::write(lang.join("pak02_dir.vpk"), b"foreign").unwrap();
        let a = d.loose("a.vmdl_c", b"a", "models/hero.vmdl_c");
        assert_eq!(install(&[a], &d.0, "russian").unwrap(), 1);
        assert!(receipt_present(&d.0, "russian").unwrap());
        let installed = lang.join("pak03_dir.vpk");
        assert_eq!(
            verified_installation(&d.0, "russian").unwrap(),
            (installed.clone(), 1)
        );
        assert_eq!(inspect(&installed).unwrap(), ["models/hero.vmdl_c"]);
        let old = fs::read(&installed).unwrap();
        let b = d.loose("b.vsnd_c", b"b", "sounds/win.vsnd_c");
        install(&[b], &d.0, "russian").unwrap();
        assert_eq!(inspect(&installed).unwrap(), ["sounds/win.vsnd_c"]);
        let current = fs::read(&installed).unwrap();
        fs::write(&installed, old).unwrap();
        assert!(verified_installation(&d.0, "russian").is_err());
        fs::write(&installed, current).unwrap();
        uninstall(&d.0, "russian").unwrap();
        assert!(!receipt_present(&d.0, "russian").unwrap());
        assert!(!installed.exists());
        assert!(verified_installation(&d.0, "russian").is_err());
        assert_eq!(fs::read(lang.join("pak02_dir.vpk")).unwrap(), b"foreign");
    }
    #[test]
    fn receipt_probe_reports_damage_and_rejects_invalid_language() {
        let d = Sandbox::new();
        assert!(receipt_present(&d.0, "../outside").is_err());
        let dir = d.path("game/dota_russian");
        fs::write(dir.join(STATE), "broken").unwrap();
        assert!(receipt_present(&d.0, "russian").is_err());
        assert_eq!(fs::read(dir.join(STATE)).unwrap(), b"broken");
        assert!(!dir.join(LOCK).exists());
    }
    #[test]
    fn receipt_requires_real_payload_of_recorded_size_for_status() {
        let d = Sandbox::new();
        let language = d.path("game/dota_russian");
        assert!(!receipt_has_payload(&d.0, "russian").unwrap());
        fs::write(language.join(STATE), "rift-atelier/1 23 - 5-11aa22bb").unwrap();
        assert!(!receipt_has_payload(&d.0, "russian").unwrap());
        let path = language.join("pak23_dir.vpk");
        fs::write(&path, b"123").unwrap();
        assert!(!receipt_has_payload(&d.0, "russian").unwrap());
        fs::write(&path, b"12345").unwrap();
        assert!(receipt_has_payload(&d.0, "russian").unwrap());
        assert!(!language.join(LOCK).exists());
    }
    #[test]
    fn refuses_to_remove_or_replace_externally_changed_files() {
        let d = Sandbox::new();
        let p = d.path("game/dota_russian/pak02_dir.vpk");
        let a = d.loose("a.vsnd_c", b"a", "sounds/ability.vsnd_c");
        install(&[a], &d.0, "russian").unwrap();
        fs::write(&p, b"new owner").unwrap();
        let b = d.loose("b.vmdl_c", b"b", "models/file.vmdl_c");
        assert!(install(&[b], &d.0, "russian").is_err());
        assert!(uninstall(&d.0, "russian").is_err());
        assert_eq!(fs::read(p).unwrap(), b"new owner");
    }
}
