use crate::{
    catalog, packages, previews, resources,
    store::{Item, Profile, ext, source_type, uid, validate_target},
};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn fail(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn import(dir: &Path, root: &Path, source: &Path, target: &str) -> Result<Vec<Item>, String> {
    catalog::check_source(source)?;
    let id = uid();
    let folder = dir.join("assets").join(&id);
    fs::create_dir_all(&folder).map_err(fail)?;
    let result = (|| {
        let input_ext = ext(source);
        let (kind, out_ext) = source_type(&input_ext).ok_or("Формат файла не поддерживается.")?;
        if out_ext.is_empty() {
            let paths = if input_ext == "zip" {
                catalog::extract_vpks(source, &folder)?
            } else {
                let dest = folder.join(source.file_name().ok_or("У файла нет имени.")?);
                fs::copy(source, &dest).map_err(fail)?;
                vec![dest]
            };
            let mut items = vec![];
            let archive_name = source.file_stem().unwrap_or_default().to_string_lossy();
            let multiple = paths.len() > 1;
            for (index, path) in paths.into_iter().enumerate() {
                let count = packages::verify_import(&path)
                    .map_err(|error| format!("«{}». {error}", shortname(&path)))?;
                let name = if multiple {
                    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                    let variant = stem.split_once('-').map_or(stem.as_ref(), |(_, name)| name);
                    format!("{archive_name} · {}. {variant}", index + 1)
                } else {
                    archive_name.to_string()
                };
                items.push(Item {
                    id: uid(),
                    name,
                    kind: kind.into(),
                    target: String::new(),
                    size: fs::metadata(&path).map_err(fail)?.len(),
                    count,
                    cover: previews::discover(source, &path, path.parent().unwrap_or(&folder))
                        .ok()
                        .flatten(),
                    file: path,
                    preview: None,
                });
            }
            Ok(items)
        } else {
            let target = target.trim().replace('\\', "/").to_ascii_lowercase();
            validate_target(&target, out_ext)?;
            let dest = folder.join(format!("asset.{out_ext}"));
            resources::compile_source(root, source, &target, &dest)?;
            let preview = if matches!(input_ext.as_str(), "wav" | "png" | "jpg" | "jpeg" | "tga") {
                let f = folder.join(format!("source.{input_ext}"));
                fs::copy(source, &f).map_err(fail)?;
                Some(f)
            } else {
                None
            };
            Ok(vec![Item {
                id: id.clone(),
                name: source
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                kind: kind.into(),
                target,
                file: dest.clone(),
                cover: if matches!(input_ext.as_str(), "vmdl" | "vmdl_c" | "vmat_c" | "vtex_c") {
                    previews::discover(source, &dest, &folder).ok().flatten()
                } else if matches!(input_ext.as_str(), "png" | "jpg" | "jpeg" | "tga") {
                    previews::attach(source, &folder)
                        .ok()
                        .map(|path| (path, "Исходная текстура".into()))
                } else {
                    None
                },
                size: fs::metadata(dest).map_err(fail)?.len(),
                count: 1,
                preview,
            }])
        }
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&folder);
    }
    result
}
pub fn inputs(profile: &Profile, items: &[Item]) -> Result<Vec<packages::Input>, String> {
    if profile.ids.is_empty() {
        return Err("Включите хотя бы один ресурс в библиотеке.".into());
    }
    profile
        .ids
        .iter()
        .map(|id| {
            let item = items
                .iter()
                .find(|i| i.id == *id)
                .ok_or("В профиле есть отсутствующий ресурс.")?;
            if !item.file.is_file() {
                return Err(format!("Файл «{}» отсутствует в библиотеке.", item.name));
            }
            if item.target.is_empty() {
                Ok(packages::Input::Vpk(item.file.clone()))
            } else {
                Ok(packages::Input::Loose {
                    source: item.file.clone(),
                    target: item.target.clone(),
                })
            }
        })
        .collect()
}
pub fn texture_preview(p: &Path) -> Result<eframe::egui::ColorImage, String> {
    let mut r = image::ImageReader::open(p)
        .map_err(fail)?
        .with_guessed_format()
        .map_err(fail)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(260 * 1024 * 1024);
    r.limits(limits);
    Ok(crate::theme::image_data(
        r.decode().map_err(fail)?.thumbnail(768, 480),
    ))
}
pub fn shortname(p: &Path) -> String {
    p.file_name().unwrap_or_default().to_string_lossy().into()
}
pub fn cleanup_removed(data: &Path, removed: &Item, remaining: &[Item]) {
    let root = data.join("assets");
    let Ok(relative) = removed.file.strip_prefix(&root) else {
        return;
    };
    if !relative
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return;
    }
    let Some(folder) = relative.iter().next().and_then(|c| c.to_str()) else {
        return;
    };
    if folder.split('-').count() != 3
        || !folder
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return;
    }
    let owned = root.join(folder);
    if remaining.iter().any(|i| i.file == removed.file) {
        return;
    }
    let _ = fs::remove_file(&removed.file);
    if let Some((cover, _)) = &removed.cover
        && cover.parent() == removed.file.parent()
        && !remaining
            .iter()
            .any(|i| i.cover.as_ref().is_some_and(|(p, _)| p == cover))
    {
        let _ = fs::remove_file(cover);
    }
    if let Some(preview) = &removed.preview
        && preview.parent() == Some(&owned)
    {
        let _ = fs::remove_file(preview);
    }
    // ZIP entries occupy one additional import directory. Remove only empty directories.
    if let Some(parent) = removed.file.parent()
        && parent.parent() == Some(&owned)
    {
        let _ = fs::remove_dir(parent);
    }
    let _ = fs::remove_dir(owned);
}
pub fn get_files(p: &Path) -> Result<Vec<PathBuf>, String> {
    catalog::recent_downloads(p, 16)
}
