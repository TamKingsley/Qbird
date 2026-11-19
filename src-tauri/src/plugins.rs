//! Local preview plugins.
//!
//! A plugin is a folder with `plugin.json` plus one self-contained HTML entry per
//! extension. Entries run inside a sandboxed iframe: no same-origin access, no
//! network, and they receive only the file bytes they were asked to preview — never
//! paths, peers, identity or database access. v1 is read-only: nothing is written back.
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const API_VERSION: u8 = 1;
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
pub const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_PLUGIN_FILES: usize = 200;
pub const MAX_PLUGIN_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_DEPTH: usize = 4;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    name: String,
    version: String,
    api: u8,
    preview: Vec<PreviewRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewRule {
    extensions: Vec<String>,
    entry: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: u8,
    pub enabled: bool,
    pub extensions: Vec<String>,
}

fn valid_id(id: &str) -> bool {
    id.len() >= 2 && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_'))
        && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn valid_extension(ext: &str) -> bool {
    !ext.is_empty() && ext.len() <= 16 && ext.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Entry must stay inside the plugin folder and be a small, regular HTML file.
fn valid_entry(dir: &Path, entry: &str) -> bool {
    if entry.is_empty() || entry.len() > 200 || entry.starts_with('/') || entry.starts_with('\\') { return false; }
    let lowered = entry.to_ascii_lowercase();
    if !(lowered.ends_with(".html") || lowered.ends_with(".htm")) { return false; }
    if entry.split(['/', '\\']).any(|part| part.is_empty() || part == "." || part == "..") { return false; }
    match fs::symlink_metadata(dir.join(entry)) {
        Ok(meta) => meta.is_file() && meta.len() > 0 && meta.len() <= MAX_ENTRY_BYTES,
        Err(_) => false,
    }
}

fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let path = dir.join("plugin.json");
    let meta = fs::metadata(&path).map_err(|_| "插件目录缺少 plugin.json".to_owned())?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_MANIFEST_BYTES {
        return Err("plugin.json 缺失或过大".into());
    }
    let text = fs::read_to_string(&path).map_err(|_| "plugin.json 不是 UTF-8 文本".to_owned())?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|_| "plugin.json 格式无效".to_owned())?;
    if !valid_id(&manifest.id) { return Err("插件 id 只能使用小写字母、数字、点、横线和下划线".into()); }
    if manifest.name.trim().is_empty() || manifest.name.chars().count() > 64
        || manifest.name.chars().any(char::is_control) {
        return Err("插件名称需为 1–64 个可见字符".into());
    }
    if manifest.version.is_empty() || manifest.version.len() > 32
        || !manifest.version.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+')) {
        return Err("插件版本号无效".into());
    }
    if manifest.api != API_VERSION { return Err(format!("插件要求 api {API_VERSION}，当前应用不支持")); }
    if manifest.preview.is_empty() || manifest.preview.len() > 8 { return Err("预览规则需为 1–8 条".into()); }
    let mut seen = std::collections::BTreeSet::new();
    for rule in &manifest.preview {
        if rule.extensions.is_empty() || rule.extensions.len() > 8 { return Err("每条规则需 1–8 个扩展名".into()); }
        for extension in &rule.extensions {
            if !valid_extension(extension) { return Err(format!("扩展名无效：{extension}")); }
            if !seen.insert(extension.clone()) { return Err(format!("扩展名重复：{extension}")); }
        }
        if !valid_entry(dir, &rule.entry) { return Err(format!("入口文件无效或不存在：{}", rule.entry)); }
    }
    Ok(manifest)
}

/// Recursively copies a plugin folder with hard limits; symlinks are rejected.
fn copy_tree(source: &Path, target: &Path, depth: usize, files: &mut usize, bytes: &mut u64) -> Result<(), String> {
    if depth > MAX_DEPTH { return Err("插件目录层级过深".into()); }
    fs::create_dir_all(target).map_err(|_| "无法创建插件目录".to_owned())?;
    let entries = fs::read_dir(source).map_err(|_| "无法读取插件目录".to_owned())?;
    for entry in entries {
        let entry = entry.map_err(|_| "无法读取插件目录".to_owned())?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| "无法读取插件文件".to_owned())?;
        if meta.file_type().is_symlink() { return Err("插件不能包含符号链接".into()); }
        let name = entry.file_name();
        let next = target.join(&name);
        if meta.is_dir() {
            copy_tree(&path, &next, depth + 1, files, bytes)?;
        } else if meta.is_file() {
            *files += 1;
            *bytes += meta.len();
            if *files > MAX_PLUGIN_FILES || *bytes > MAX_PLUGIN_BYTES {
                return Err(format!("插件超过 {MAX_PLUGIN_FILES} 个文件或 16 MiB 限制"));
            }
            fs::copy(&path, &next).map_err(|_| "无法复制插件文件".to_owned())?;
        } else {
            return Err("插件只能包含普通文件与目录".into());
        }
    }
    Ok(())
}

pub fn plugins_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plugins")
}

/// Validates a folder and copies it to `<data>/plugins/<id>`. Refuses to overwrite.
pub fn install(data_dir: &Path, source: &Path) -> Result<(PluginView, PathBuf), String> {
    let manifest = read_manifest(source)?;
    let root = plugins_dir(data_dir);
    fs::create_dir_all(&root).map_err(|_| "无法创建插件目录".to_owned())?;
    let target = root.join(&manifest.id);
    if target.exists() { return Err("已安装同名插件，请先移除后再安装".into()); }
    let mut files = 0usize;
    let mut bytes = 0u64;
    copy_tree(source, &target, 0, &mut files, &mut bytes)?;
    // Re-validate the copy so a partially failed install never registers.
    let manifest = match read_manifest(&target) {
        Ok(manifest) => manifest,
        Err(error) => { let _ = fs::remove_dir_all(&target); return Err(error); }
    };
    Ok((view(&manifest, true), target))
}

fn view(manifest: &Manifest, enabled: bool) -> PluginView {
    PluginView {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        api: manifest.api,
        enabled,
        extensions: manifest.preview.iter().flat_map(|rule| rule.extensions.clone()).collect(),
    }
}

pub fn load_view(dir: &Path, enabled: bool) -> Option<PluginView> {
    read_manifest(dir).ok().map(|manifest| view(&manifest, enabled))
}

/// Removes an installed plugin folder. Only paths inside the plugins root are touched.
pub fn remove(data_dir: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) { return Err("插件 id 无效".into()); }
    let root = plugins_dir(data_dir);
    let target = root.join(id);
    if !target.starts_with(&root) || target.parent() != Some(root.as_path()) { return Err("插件路径无效".into()); }
    match fs::remove_dir_all(&target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法删除插件目录".to_owned()),
    }
}

/// Finds an enabled plugin rule for `file_name` and returns its entry HTML.
pub fn entry_html(data_dir: &Path, file_name: &str, is_enabled: impl Fn(&str) -> bool) -> Option<(String, String)> {
    let extension = file_name.rsplit_once('.')?.1.to_ascii_lowercase();
    if !valid_extension(&extension) { return None; }
    let root = plugins_dir(data_dir);
    let mut candidates: Vec<PathBuf> = fs::read_dir(&root).ok()?
        .filter_map(|entry| entry.ok().map(|item| item.path())).collect();
    candidates.sort();
    for dir in candidates {
        if !dir.is_dir() { continue; }
        let Some(manifest) = read_manifest(&dir).ok() else { continue };
        if !is_enabled(&manifest.id) { continue; }
        for rule in &manifest.preview {
            if rule.extensions.iter().any(|item| item == &extension) && valid_entry(&dir, &rule.entry) {
                if let Ok(html) = fs::read_to_string(dir.join(&rule.entry)) {
                    return Some((manifest.id, html));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "qbird-plugin-{label}-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|v| v.as_nanos()).unwrap_or(0)));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_plugin(dir: &Path, id: &str, html: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("plugin.json"), format!(
            r#"{{"id":"{id}","name":"演示","version":"1.0.0","api":1,"preview":[{{"extensions":["docx"],"entry":"preview.html"}}]}}"#)).unwrap();
        fs::write(dir.join("preview.html"), html).unwrap();
    }

    #[test]
    fn installs_lists_and_removes_a_plugin() {
        let data = temp_dir("data");
        let source = temp_dir("src");
        write_plugin(&source, "demo.preview", "<h1>hi</h1>");
        let (view, dir) = install(&data, &source).unwrap();
        assert_eq!(view.id, "demo.preview");
        assert_eq!(view.extensions, vec!["docx".to_owned()]);
        assert!(dir.starts_with(plugins_dir(&data)));
        // Installing the same id again must not overwrite.
        assert!(install(&data, &source).is_err());
        assert!(entry_html(&data, "报告.docx", |_| true).is_some());
        assert!(entry_html(&data, "报告.docx", |_| false).is_none());
        assert!(entry_html(&data, "报告.exe", |_| true).is_none());
        remove(&data, "demo.preview").unwrap();
        assert!(entry_html(&data, "报告.docx", |_| true).is_none());
        let _ = fs::remove_dir_all(&data);
        let _ = fs::remove_dir_all(&source);
    }

    #[test]
    fn rejects_unsafe_manifests() {
        let data = temp_dir("bad-data");
        let source = temp_dir("bad-src");
        // Entry escaping the folder.
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("plugin.json"), r#"{"id":"demo","name":"x","version":"1","api":1,"preview":[{"extensions":["pdf"],"entry":"../evil.html"}]}"#).unwrap();
        assert!(install(&data, &source).is_err());
        // Wrong api version and duplicate extensions.
        fs::write(source.join("plugin.json"), r#"{"id":"demo","name":"x","version":"1","api":2,"preview":[{"extensions":["pdf"],"entry":"preview.html"}]}"#).unwrap();
        assert!(install(&data, &source).is_err());
        fs::write(source.join("plugin.json"), r#"{"id":"demo","name":"x","version":"1","api":1,"preview":[{"extensions":["pdf","pdf"],"entry":"preview.html"}]}"#).unwrap();
        assert!(install(&data, &source).is_err());
        // Invalid ids.
        assert!(remove(&data, "../escape").is_err());
        let _ = fs::remove_dir_all(&data);
        let _ = fs::remove_dir_all(&source);
    }
}
