#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_the_order_marks_the_profile_as_needing_apply() {
        let mut state = State::default();
        state.profiles[0].ids = vec!["sound".into(), "hero".into()];
        assert!(!state.current_is_installed());
        let profile = &state.profiles[0];
        state.installed = Some(Installation {
            profile: profile.name.clone(),
            root: state.dota.clone(),
            language: state.language.clone(),
            ids: Some(profile.ids.clone()),
        });
        assert!(state.current_is_installed());
        state.profiles[0].ids.swap(0, 1);
        assert!(!state.current_is_installed());
        state.profiles[0].ids.swap(0, 1);
        state.toggle("hero");
        assert!(!state.current_is_installed());
        state.installed.as_mut().unwrap().ids = None;
        assert!(!state.current_is_installed());
    }
    #[test]
    fn version_one_items_and_profile_survive_upgrade() {
        let dir = std::env::temp_dir().join(uid());
        fs::create_dir_all(&dir).unwrap();
        let old = r#"{"items":[{"id":"v1","name":"Старый мод","kind":"Пакет","target":"","file":"old.vpk","preview":null,"size":99,"count":1}],"profiles":[{"name":"Любимые","ids":["v1"]}],"selected":0}"#;
        fs::write(dir.join("library.json"), old).unwrap();
        let library = State::load(&dir).unwrap();
        assert!(library.items[0].cover.is_none());
        library.save(&dir).unwrap();
        let migrated = State::load(&dir).unwrap();
        assert_eq!(migrated.items[0].name, "Старый мод");
        assert_eq!(migrated.profiles[0].ids, ["v1"]);
        assert_eq!(migrated.profiles[0].name, "Любимые");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn profile_order_and_disable_survive_restart() {
        let mut state = State::default();
        state.items = vec![Item::example("a"), Item::example("b"), Item::example("c")];
        state.profiles[0].ids = vec!["b".into(), "c".into(), "a".into()];
        state.profiles.push(Profile {
            name: "Демо".into(),
            ids: vec!["a".into()],
        });
        state.toggle("b");
        let dir = std::env::temp_dir().join(uid());
        state.save(&dir).unwrap();
        let reloaded = State::load(&dir).unwrap();
        assert_eq!(reloaded.profiles[0].ids, ["c", "a"]);
        assert_eq!(reloaded.profiles[1].ids, ["a"]);
        assert_eq!(reloaded.items.len(), 3);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn recovers_installation_across_interrupted_library_commit() {
        let root = std::env::temp_dir().join(uid());
        let game = root.join("game");
        let lang = game.join("dota_koreana");
        fs::create_dir_all(&lang).unwrap();
        fs::create_dir(game.join("dota")).unwrap();
        fs::write(game.join("dota/pak01_dir.vpk"), []).unwrap();
        let receipt = lang.join(".rift-atelier.install");
        fs::write(&receipt, "rift-atelier/1 20 - 38-ab12cd34").unwrap();
        let mut state = State {
            dota: root.clone(),
            ..Default::default()
        };
        recover_installation(&mut state).unwrap();
        assert_eq!(state.installed.as_ref().unwrap().language, "koreana");
        fs::remove_file(receipt).unwrap();
        recover_installation(&mut state).unwrap();
        assert!(state.installed.is_none());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn accepts_only_relative_compatible_targets() {
        assert!(validate_target("models/heroes/axe/axe.vmdl_c", "vmdl_c").is_ok());
        for path in [
            "../x.vmdl_c",
            "C:/x.vmdl_c",
            "/etc/model.vmdl_c",
            "models/../hero.vmdl_c",
            "models/a.vtex_c",
            "models//a.vmdl_c",
        ] {
            assert!(validate_target(path, "vmdl_c").is_err(), "{path}");
        }
    }
}

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub target: String,
    pub file: PathBuf,
    pub preview: Option<PathBuf>,
    #[serde(default)]
    pub cover: Option<(PathBuf, String)>,
    pub size: u64,
    pub count: usize,
}
impl Item {
    #[cfg(test)]
    fn example(id: &str) -> Self {
        Self {
            id: id.into(),
            name: id.into(),
            kind: "Пакет".into(),
            target: String::new(),
            file: PathBuf::new(),
            preview: None,
            cover: None,
            size: 100,
            count: 1,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub ids: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Installation {
    pub profile: String,
    pub root: PathBuf,
    pub language: String,
    #[serde(default)]
    pub ids: Option<Vec<String>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub bkb_volume: u8,
    pub items: Vec<Item>,
    pub profiles: Vec<Profile>,
    pub selected: usize,
    pub dota: PathBuf,
    pub language: String,
    pub installed: Option<Installation>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            bkb_volume: 13,
            items: vec![],
            profiles: vec![Profile {
                name: "Основной".into(),
                ids: vec![],
            }],
            selected: 0,
            dota: PathBuf::new(),
            language: "russian".into(),
            installed: None,
        }
    }
}
impl State {
    pub fn current_is_installed(&self) -> bool {
        let Some(profile) = self.profiles.get(self.selected) else {
            return false;
        };
        self.installed.as_ref().is_some_and(|i| {
            i.root == self.dota
                && i.language == self.language
                && i.profile == profile.name
                && i.ids.as_deref() == Some(profile.ids.as_slice())
        })
    }
    pub fn toggle(&mut self, id: &str) {
        let ids = &mut self.profiles[self.selected].ids;
        if ids.iter().any(|s| s == id) {
            ids.retain(|s| s != id)
        } else {
            ids.push(id.into())
        }
    }
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("library.json");
        let mut state: Self = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("Не удалось прочитать библиотеку: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.to_string()),
        };
        if state.profiles.is_empty() {
            state.profiles = Self::default().profiles
        }
        state.selected = state.selected.min(state.profiles.len() - 1);
        Ok(state)
    }
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let tmp = dir.join(format!("{}.tmp", uid()));
        let result = (|| -> std::io::Result<()> {
            fs::create_dir_all(dir)?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            file.write_all(&serde_json::to_vec_pretty(self)?)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&tmp, dir.join("library.json"))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result.map_err(|e| format!("Сохранение библиотеки: {e}"))
    }
}
pub fn data_dir() -> PathBuf {
    std::env::var_os("RIFT_ATELIER_DATA")
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(|v| PathBuf::from(v).join("RiftAtelier").into_os_string())
        })
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("RiftAtelier"))
}
pub fn uid() -> String {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    format!(
        "{:x}-{:x}-{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}
pub fn ext(p: &Path) -> String {
    p.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase()
}
pub fn fmt_size(n: u64) -> String {
    if n >= 1_048_576 {
        format!("{:.1} МБ", n as f64 / 1_048_576.)
    } else if n >= 1024 {
        format!("{} КБ", n.div_ceil(1024))
    } else {
        format!("{n} Б")
    }
}
pub fn source_type(ext: &str) -> Option<(&'static str, &'static str)> {
    Some(match ext {
        "png" | "jpg" | "jpeg" | "tga" | "vtex_c" => ("Текстура", "vtex_c"),
        "vmat_c" => ("Материал", "vmat_c"),
        "vmdl" | "vmdl_c" => ("Модель", "vmdl_c"),
        "wav" | "vsnd_c" => ("Звук", "vsnd_c"),
        "vpk" | "zip" => ("Пакет", ""),
        _ => return None,
    })
}
pub fn validate_target(path: &str, extension: &str) -> Result<(), String> {
    let clean = path.replace('\\', "/");
    let lower = clean.to_ascii_lowercase();
    if !lower.ends_with(&format!(".{extension}"))
        || !clean.contains('/')
        || clean.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with('.')
                || part.ends_with(' ')
                || part
                    .chars()
                    .any(|c| c.is_control() || "<>:\"|?*".contains(c))
        })
    {
        Err(format!(
            "Нужен относительный игровой путь с расширением .{extension}."
        ))
    } else {
        Ok(())
    }
}
pub fn game_ok(p: &Path) -> bool {
    p.join("game/dota/pak01_dir.vpk").is_file()
}
pub fn suggest_target(source: &Path, extension: &str) -> Option<String> {
    if ext(source) != extension || !extension.ends_with("_c") {
        return None;
    }
    let normalized = source.to_string_lossy().replace('\\', "/");
    let parts: Vec<_> = normalized.split('/').collect();
    let start = parts.iter().position(|part| {
        ["sounds", "models", "materials", "panorama"]
            .iter()
            .any(|prefix| part.eq_ignore_ascii_case(prefix))
    })?;
    let target = parts[start..].join("/").to_ascii_lowercase();
    validate_target(&target, extension).ok()?;
    Some(target)
}
pub fn detect_game() -> Option<PathBuf> {
    let mut steam = vec![];
    if let Some(p) = std::env::var_os("PROGRAMFILES(X86)") {
        steam.push(PathBuf::from(p).join("Steam"));
    }
    if let Some(p) = crate::native::steam_path() {
        steam.push(p);
    }
    let mut roots = steam.clone();
    for root in steam {
        if let Ok(vdf) = fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            for line in vdf.lines() {
                let fields: Vec<_> = line.split('"').collect();
                if fields.get(1) == Some(&"path")
                    && let Some(val) = fields.get(3)
                {
                    roots.push(PathBuf::from(val.replace("\\\\", "\\")));
                }
            }
        }
    }
    roots
        .into_iter()
        .map(|r| r.join("steamapps/common/dota 2 beta"))
        .find(|p| game_ok(p))
}

pub fn recover_installation(state: &mut State) -> Result<(), String> {
    // The game-side receipt is committed separately from the library.
    if let Some(old) = &mut state.installed {
        if !game_ok(&old.root) {
            old.ids = None;
            return Ok(());
        }
        if crate::packages::receipt_present(&old.root, &old.language)? {
            if !crate::packages::receipt_has_payload(&old.root, &old.language)? {
                old.ids = None;
            }
            return Ok(());
        }
        state.installed = None;
    }
    if game_ok(&state.dota) {
        for lang in [&state.language[..], "russian", "koreana", "schinese"] {
            if crate::packages::receipt_present(&state.dota, lang)? {
                state.installed = Some(Installation {
                    profile: "Установленный пакет Rift Atelier".into(),
                    root: state.dota.clone(),
                    language: lang.to_string(),
                    ids: None,
                });
                break;
            }
        }
    }
    Ok(())
}
