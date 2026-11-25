use crate::{
    library, packages,
    store::{self, Installation, State},
};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

/// Dota records voice and UI language separately in this generated KeyValues file.
pub fn audio_language(root: &Path) -> Option<String> {
    let mut config = String::new();
    File::open(root.join("game/dota/cfg/boot.vcfg"))
        .ok()?
        .take(65537)
        .read_to_string(&mut config)
        .ok()?;
    if config.len() > 65536 {
        return None;
    }
    let mut tokens = config.lines().flat_map(|line| {
        let code = line.split("//").next().unwrap_or("");
        code.split('"').skip(1).step_by(2)
    });
    while let Some(token) = tokens.next() {
        if token.eq_ignore_ascii_case("AudioLanguage") {
            let language = tokens.next()?.trim();
            if !language.is_empty()
                && language
                    .bytes()
                    .all(|b| b.is_ascii_alphabetic() || b == b'_')
            {
                return Some(language.to_ascii_lowercase());
            }
        }
    }
    None
}

pub fn apply(state: &State) -> Result<(Installation, usize, PathBuf), String> {
    let profile = state
        .profiles
        .get(state.selected)
        .ok_or("Выберите профиль в библиотеке.")?;
    let inputs = library::inputs(profile, &state.items)?;
    if !store::game_ok(&state.dota) {
        return Err(
            "Укажите папку Dota 2 в настройках: в выбранной папке нет game/dota/pak01_dir.vpk."
                .into(),
        );
    }
    if let Some(audio) = audio_language(&state.dota)
        && audio != state.language
    {
        let fix = if matches!(audio.as_str(), "russian" | "koreana" | "schinese") {
            "Выберите в настройках Rift Atelier язык озвучки игры и примените профиль снова."
        } else {
            "Для этой озвучки доступен экспорт VPK. Для установки через профиль доступны русская, корейская и упрощённая китайская озвучки."
        };
        return Err(format!(
            "Озвучка Dota 2 — {audio}, в Rift Atelier выбрана {}. {fix}",
            state.language
        ));
    }
    if let Some(old) = &state.installed
        && (old.root != state.dota || old.language != state.language)
        && packages::receipt_present(&old.root, &old.language)?
    {
        return Err(
            "Снимите ранее установленный профиль в настройках перед сменой папки игры или озвучки."
                .into(),
        );
    }
    let (path, verified) = packages::install_verified(&inputs, &state.dota, &state.language)?;
    let record = Installation {
        profile: profile.name.clone(),
        root: state.dota.clone(),
        language: state.language.clone(),
        ids: Some(profile.ids.clone()),
    };
    Ok((record, verified, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Item, Profile};
    use std::fs;

    struct Sandbox(PathBuf);
    impl Sandbox {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("rift-deployment-{}", crate::store::uid()));
            fs::create_dir_all(root.join("game/dota/cfg")).unwrap();
            fs::create_dir(root.join("game/dota_russian")).unwrap();
            fs::write(root.join("game/dota/gameinfo.gi"), []).unwrap();
            fs::write(root.join("game/dota/pak01_dir.vpk"), []).unwrap();
            Self(root)
        }
        fn config(&self, config: &str) {
            fs::write(self.0.join("game/dota/cfg/boot.vcfg"), config).unwrap();
        }
        fn state(&self) -> State {
            let mut items = Vec::new();
            for (id, data) in [("a", &b"first"[..]), ("b", &b"second"[..])] {
                let file = self.0.join(format!("{id}.vtex_c"));
                fs::write(&file, data).unwrap();
                items.push(Item {
                    id: id.into(),
                    name: id.into(),
                    kind: "Текстура".into(),
                    target: "materials/hero.vtex_c".into(),
                    file,
                    preview: None,
                    cover: None,
                    size: data.len() as u64,
                    count: 1,
                });
            }
            State {
                dota: self.0.clone(),
                language: "russian".into(),
                items,
                profiles: vec![Profile {
                    name: "Герои".into(),
                    ids: vec!["b".into(), "a".into()],
                }],
                ..State::default()
            }
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn applies_and_verifies_ordered_profile_with_or_without_boot_config() {
        for config in [
            None,
            Some(
                "\u{feff}\"boot\" {\r\n// \"AudioLanguage\" \"english\"\r\n\"UILanguage\" \"english\"\r\n\"AudioLanguage\" \"Russian\"\r\n}",
            ),
        ] {
            let d = Sandbox::new();
            let state = d.state();
            if let Some(config) = config {
                d.config(config);
            }
            let (record, count, path) = apply(&state).unwrap();
            assert_eq!(count, 1);
            assert_eq!(record.profile, "Герои");
            assert_eq!(
                record.ids.as_deref(),
                Some(state.profiles[0].ids.as_slice())
            );
            assert_eq!(record.language, "russian");
            assert_eq!(path.parent(), Some(d.0.join("game/dota_russian").as_path()));
            assert_eq!(
                packages::read_member(&path, "materials/hero.vtex_c", 16).unwrap(),
                b"first"
            );
        }
    }
    #[test]
    fn mismatch_explains_audio_language_and_keeps_game_unmodified() {
        let d = Sandbox::new();
        let state = d.state();
        d.config(r#""boot" { "UILanguage" "russian" "AudioLanguage" "english" }"#);
        let error = apply(&state).err().unwrap();
        assert!(
            error.contains("english") && error.contains("russian"),
            "{error}"
        );
        assert_eq!(
            fs::read_dir(d.0.join("game/dota_russian")).unwrap().count(),
            0
        );
    }
    #[test]
    fn missing_library_file_keeps_previous_verified_package() {
        let d = Sandbox::new();
        let state = d.state();
        let (_, _, installed) = apply(&state).unwrap();
        let old = fs::read(&installed).unwrap();
        fs::remove_file(&state.items[0].file).unwrap();
        let error = apply(&state).err().unwrap();
        assert!(error.contains("отсутствует"), "{error}");
        assert_eq!(fs::read(installed).unwrap(), old);
    }
}
