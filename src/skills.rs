use crate::{Ctx, Reply};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Cursor, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Mutex, MutexGuard},
};
use zip::ZipArchive;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

static SKILL_IO: Mutex<()> = Mutex::new(());

const MAX_ARCHIVE: u64 = 32 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 128 * 1024 * 1024;
const MAX_FILE: u64 = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 1024;
const MAX_MANIFEST: u64 = 512 * 1024;
const MAX_READ: u64 = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct Manifest {
    name: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct SkillInfo {
    name: String,
    description: String,
    enabled: bool,
}

fn arg<'a>(a: &'a Value, key: &str) -> Result<&'a str, String> {
    a[key].as_str().ok_or_else(|| format!("缺少参数 {key}"))
}

fn reserved(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("Skill 名称长度必须为 1–64 个字符".into());
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || reserved(name)
    {
        return Err(
            "Skill 名称只能使用小写字母、数字和单个连字符，且不能以连字符开头或结尾".into(),
        );
    }
    Ok(())
}

fn relative_path(raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty()
        || raw.starts_with('/')
        || raw.contains('\\')
        || raw.chars().any(char::is_control)
    {
        return Err("Skill 包中包含不安全路径".into());
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err("Skill 包中包含绝对路径".into());
    }
    for component in path.components() {
        let Component::Normal(part) = component else {
            return Err("Skill 包中包含不安全路径".into());
        };
        let part = part.to_string_lossy();
        if part.contains(':') || part.ends_with('.') || part.ends_with(' ') || reserved(&part) {
            return Err(format!("Skill 包中包含不兼容的路径 {part}"));
        }
    }
    Ok(path.to_path_buf())
}

fn skill_guard() -> Result<MutexGuard<'static, ()>, String> {
    SKILL_IO.lock().map_err(|_| "Skill 文件操作锁已失效".into())
}

#[cfg(unix)]
fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(windows)]
fn windows_file_identity(file: &File) -> Result<(u32, u32, u32), String> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    if ok == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok((
        info.dwVolumeSerialNumber,
        info.nFileIndexHigh,
        info.nFileIndexLow,
    ))
}

#[cfg(unix)]
fn opened_file_matches_path(file: &File, path: &Path) -> Result<bool, String> {
    let opened = file.metadata().map_err(|e| e.to_string())?;
    let current = fs::metadata(path).map_err(|e| e.to_string())?;
    Ok(same_file(&opened, &current))
}

#[cfg(windows)]
fn opened_file_matches_path(file: &File, path: &Path) -> Result<bool, String> {
    let current = File::open(path).map_err(|e| e.to_string())?;
    Ok(windows_file_identity(file)? == windows_file_identity(&current)?)
}

fn read_limited(file: &mut File, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut *file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("Skill 文件超过 {} KiB", limit / 1024));
    }
    Ok(bytes)
}

fn read_text_within(path: &Path, root: &Path, limit: u64) -> Result<String, String> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let before = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err("Skill 文件不存在或不是普通文件".into());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("Skill 文件不存在或不是普通文件".into());
    }
    if meta.len() > limit {
        return Err(format!("Skill 文本文件超过 {} KiB", limit / 1024));
    }
    let after = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if after.file_type().is_symlink() {
        return Err("Skill 路径不能包含符号链接".into());
    }
    let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !canonical.starts_with(&root) {
        return Err("Skill 路径超出允许范围".into());
    }
    if !opened_file_matches_path(&file, &canonical)? {
        return Err("Skill 文件在读取过程中已更改".into());
    }
    let bytes = read_limited(&mut file, limit)?;
    String::from_utf8(bytes).map_err(|_| "Skill 文本必须是 UTF-8 编码".into())
}

fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return Err("SKILL.md 必须以 YAML frontmatter 开始".into());
    }
    let mut yaml = Vec::new();
    let mut body = Vec::new();
    let mut closed = false;
    for line in lines {
        if !closed && line == "---" {
            closed = true;
        } else if closed {
            body.push(line);
        } else {
            yaml.push(line);
        }
    }
    if !closed {
        return Err("SKILL.md 的 YAML frontmatter 缺少结束标记".into());
    }
    if body.iter().all(|line| line.trim().is_empty()) {
        return Err("SKILL.md 必须包含 Skill 说明正文".into());
    }
    let manifest: Manifest = serde_yaml::from_str(&yaml.join("\n"))
        .map_err(|e| format!("SKILL.md frontmatter 无效：{e}"))?;
    validate_name(&manifest.name)?;
    if manifest.description.trim().is_empty()
        || manifest.description.chars().count() > 1024
        || manifest.description.contains('\r')
        || manifest.description.contains('\n')
    {
        return Err("Skill 描述必须为单行且不超过 1024 个字符".into());
    }
    Ok(manifest)
}

fn manifest_at(dir: &Path) -> Result<Manifest, String> {
    parse_manifest(&read_text_within(&dir.join("SKILL.md"), dir, MAX_MANIFEST)?)
}

fn skills_root(ctx: &Ctx) -> Result<PathBuf, String> {
    let data_root = fs::canonicalize(&ctx.root).map_err(|e| e.to_string())?;
    let root = data_root.join("skills");
    match fs::symlink_metadata(&root) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err("Skill 数据目录无效".into());
        }
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&root).map_err(|e| e.to_string())?;
        }
        Err(e) => return Err(e.to_string()),
    }
    let canonical = fs::canonicalize(&root).map_err(|e| e.to_string())?;
    if canonical != root || canonical.parent() != Some(data_root.as_path()) {
        return Err("Skill 数据目录超出允许范围".into());
    }
    Ok(canonical)
}

type SkillState = BTreeMap<String, bool>;

fn state(ctx: &Ctx) -> Result<SkillState, String> {
    let value = ctx.read("skills", json!({}))?;
    let object = value
        .as_object()
        .ok_or_else(|| "skills.json 格式错误".to_string())?;
    object
        .iter()
        .map(|(name, enabled)| {
            enabled
                .as_bool()
                .map(|value| (name.clone(), value))
                .ok_or_else(|| "skills.json 的启停状态必须为布尔值".to_string())
        })
        .collect()
}

fn update_state(
    ctx: &Ctx,
    f: impl FnOnce(&mut serde_json::Map<String, Value>) -> Result<(), String>,
) -> Reply {
    state(ctx)?;
    ctx.update("skills", json!({}), |saved| {
        let object = saved
            .as_object_mut()
            .ok_or_else(|| "skills.json 格式错误".to_string())?;
        f(object)
    })
}

fn is_enabled(state: &SkillState, name: &str) -> bool {
    state.get(name).copied().unwrap_or(true)
}

fn info(manifest: Manifest, state: &SkillState) -> SkillInfo {
    SkillInfo {
        enabled: is_enabled(state, &manifest.name),
        name: manifest.name,
        description: manifest.description,
    }
}

fn installed_at(root: &Path, name: &str) -> Result<(PathBuf, Manifest), String> {
    validate_name(name)?;
    let dir = root.join(name);
    let meta = fs::symlink_metadata(&dir).map_err(|_| format!("Skill {name} 不存在"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!("Skill {name} 目录无效"));
    }
    let canonical = fs::canonicalize(&dir).map_err(|e| e.to_string())?;
    if canonical != dir || canonical.parent() != Some(root) {
        return Err("Skill 目录超出允许范围".into());
    }
    let manifest = manifest_at(&canonical)?;
    if manifest.name != name {
        return Err("Skill 目录与 frontmatter 名称不一致".into());
    }
    Ok((canonical, manifest))
}

fn list_skills(ctx: &Ctx) -> Reply {
    let _guard = skill_guard()?;
    let root = skills_root(ctx)?;
    let state = state(ctx)?;
    let mut skills = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok((_, manifest)) = installed_at(&root, &name) else {
            continue;
        };
        skills.push(info(manifest, &state));
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(json!(skills))
}

fn find_manifests(dir: &Path, found: &mut Vec<PathBuf>, visited: &mut usize) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        *visited += 1;
        if *visited > MAX_ENTRIES {
            return Err("Skill 包内文件过多".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err("Skill 包不能包含符号链接".into());
        }
        let path = entry.path();
        if kind.is_dir() {
            find_manifests(&path, found, visited)?;
        } else if kind.is_file() && path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md") {
            found.push(path);
        }
    }
    Ok(())
}

fn zip_u16(data: &[u8], offset: usize) -> Result<u16, String> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| "Skill 压缩包偏移溢出".to_string())?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| "Skill 压缩包已截断".to_string())?;
    let mut value = [0_u8; 2];
    value.copy_from_slice(bytes);
    Ok(u16::from_le_bytes(value))
}

fn zip_u32(data: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| "Skill 压缩包偏移溢出".to_string())?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| "Skill 压缩包已截断".to_string())?;
    let mut value = [0_u8; 4];
    value.copy_from_slice(bytes);
    Ok(u32::from_le_bytes(value))
}

fn preflight_zip(data: &[u8]) -> Result<usize, String> {
    if data.len() < 22 {
        return Err("Skill 压缩包已截断".into());
    }
    let search_start = data.len().saturating_sub(22 + u16::MAX as usize);
    let mut selected = None;
    for offset in (search_start..=data.len() - 22).rev() {
        if &data[offset..offset + 4] != &[0x50, 0x4b, 0x05, 0x06] {
            continue;
        }
        let comment_len = zip_u16(data, offset + 20)? as usize;
        if offset + 22 + comment_len != data.len() {
            continue;
        }
        let disk = zip_u16(data, offset + 4)?;
        let central_disk = zip_u16(data, offset + 6)?;
        let disk_entries = zip_u16(data, offset + 8)?;
        let entries = zip_u16(data, offset + 10)?;
        let central_size = zip_u32(data, offset + 12)?;
        let central_offset = zip_u32(data, offset + 16)?;
        if disk == u16::MAX
            || central_disk == u16::MAX
            || disk_entries == u16::MAX
            || entries == u16::MAX
            || central_size == u32::MAX
            || central_offset == u32::MAX
        {
            return Err("不支持 ZIP64 Skill 压缩包".into());
        }
        if entries as usize > MAX_ENTRIES {
            return Err("Skill 包内文件过多".into());
        }
        if selected.is_none() {
            selected = Some((
                offset,
                disk,
                central_disk,
                disk_entries,
                entries,
                central_size,
            ));
        }
    }
    let (_, disk, central_disk, disk_entries, entries, central_size) =
        selected.ok_or_else(|| "Skill 压缩包缺少结束记录".to_string())?;
    if disk != 0 || central_disk != 0 || disk_entries != entries {
        return Err("Skill 压缩包不能跨磁盘".into());
    }
    if entries == 0 {
        return Err("Skill 压缩包为空".into());
    }
    if central_size as u64 > data.len() as u64 || entries as usize * 46 > central_size as usize {
        return Err("Skill 压缩包中央目录无效".into());
    }
    Ok(entries as usize)
}

fn create_dirs_limited(path: &Path, root: &Path, created: &mut usize) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Skill 包路径超出解压目录".to_string())?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err("Skill 包中包含不安全路径".into());
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err("Skill 解压路径不是普通文件夹".into());
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                *created += 1;
                if *created > MAX_ENTRIES {
                    return Err("Skill 包内文件或文件夹过多".into());
                }
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        let meta = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
                        if meta.file_type().is_symlink() || !meta.is_dir() {
                            return Err("Skill 解压路径不是普通文件夹".into());
                        }
                    }
                    Err(e) => return Err(e.to_string()),
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

fn extract_zip(data: Vec<u8>, dest: &Path) -> Result<(), String> {
    let expected = preflight_zip(&data)?;
    let mut archive =
        ZipArchive::new(Cursor::new(data)).map_err(|e| format!("无法读取 Skill 压缩包：{e}"))?;
    if archive.len() != expected {
        return Err("Skill 压缩包条目数量无效".into());
    }
    let mut seen = HashSet::new();
    let mut total = 0_u64;
    let mut created_entries = 0_usize;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let relative = relative_path(entry.name())?;
        if !seen.insert(relative.clone()) {
            return Err("Skill 包中包含重复路径".into());
        }
        let mode = entry.unix_mode();
        if mode.is_some_and(|m| !matches!(m & 0o170000, 0 | 0o100000 | 0o040000)) {
            return Err("Skill 包只能包含普通文件和文件夹".into());
        }
        let target = dest.join(&relative);
        if entry.is_dir() || mode.is_some_and(|m| m & 0o170000 == 0o040000) {
            create_dirs_limited(&target, dest, &mut created_entries)?;
            continue;
        }
        let size = entry.size();
        if size > MAX_FILE {
            return Err("Skill 包中单个文件超过 32 MiB".into());
        }
        total = total
            .checked_add(size)
            .filter(|n| *n <= MAX_EXTRACTED)
            .ok_or_else(|| "Skill 包解压后超过 128 MiB".to_string())?;
        if let Some(parent) = target.parent() {
            create_dirs_limited(parent, dest, &mut created_entries)?;
        }
        created_entries += 1;
        if created_entries > MAX_ENTRIES {
            return Err("Skill 包内文件或文件夹过多".into());
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|e| e.to_string())?;
        let copied = io::copy(&mut entry.by_ref().take(MAX_FILE + 1), &mut output)
            .map_err(|e| e.to_string())?;
        if copied > MAX_FILE {
            return Err("Skill 包中单个文件超过 32 MiB".into());
        }
        total = total
            .checked_add(copied.saturating_sub(size))
            .filter(|n| *n <= MAX_EXTRACTED)
            .ok_or_else(|| "Skill 包解压后超过 128 MiB".to_string())?;
        output.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn read_source(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("Skill 文件不存在或不是普通文件".into());
    }
    if meta.len() > limit {
        return Err("Skill 文件超过 32 MiB".into());
    }
    read_limited(&mut file, limit)
}

#[cfg(windows)]
fn install_stage(stage: &Path, dest: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let from = wide(stage);
    let to = wide(dest);
    let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn install_stage(stage: &Path, dest: &Path) -> io::Result<()> {
    fs::rename(stage, dest)
}

fn import_skill(ctx: &Ctx, source: &Path) -> Reply {
    let _guard = skill_guard()?;
    if !source.is_absolute() {
        return Err("Skill 文件路径必须是绝对路径".into());
    }
    let file_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Skill 文件名无效")?
        .to_ascii_lowercase();
    let archive = file_name.ends_with(".skill") || file_name.ends_with(".zip");
    let markdown = file_name == "skill.md";
    if !archive && !markdown {
        return Err("请选择 SKILL.md、.skill 或 .zip 文件".into());
    }
    let data = read_source(source, MAX_ARCHIVE)?;
    let previous = state(ctx)?;

    let root = skills_root(ctx)?;
    let temp = tempfile::Builder::new()
        .prefix(".skill-import-")
        .tempdir_in(&root)
        .map_err(|e| e.to_string())?;
    let extracted = temp.path().join("extracted");
    fs::create_dir(&extracted).map_err(|e| e.to_string())?;
    if archive {
        extract_zip(data, &extracted)?;
    } else {
        let text =
            String::from_utf8(data).map_err(|_| "Skill 文本必须是 UTF-8 编码".to_string())?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(extracted.join("SKILL.md"))
            .map_err(|e| e.to_string())?;
        output
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
    }

    let mut manifests = Vec::new();
    find_manifests(&extracted, &mut manifests, &mut 0)?;
    if manifests.len() != 1 {
        return Err("Skill 包中必须且只能包含一个 SKILL.md".into());
    }
    let manifest = manifest_at(manifests[0].parent().ok_or("Skill 目录无效")?)?;
    let stage = temp.path().join("skill");
    let source_dir = manifests[0].parent().ok_or("Skill 目录无效")?;
    if source_dir != extracted {
        fs::rename(source_dir, &stage).map_err(|e| e.to_string())?;
    } else {
        fs::rename(&extracted, &stage).map_err(|e| e.to_string())?;
    }

    let dest = root.join(&manifest.name);
    let reserved = if cfg!(windows) {
        None
    } else {
        fs::create_dir(&dest).map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                format!("Skill {} 已存在，请先删除再导入", manifest.name)
            } else {
                e.to_string()
            }
        })?;
        Some(dest.clone())
    };
    if let Err(error) = ctx.update("skills", json!({}), |saved| {
        saved
            .as_object_mut()
            .ok_or_else(|| "skills.json 格式错误".to_string())?
            .insert(manifest.name.clone(), json!(true));
        Ok(())
    }) {
        if let Some(path) = reserved {
            let _ = fs::remove_dir(path);
        }
        return Err(error);
    }
    if let Err(error) = install_stage(&stage, &dest) {
        let message = if error.kind() == io::ErrorKind::AlreadyExists {
            format!("Skill {} 已存在，请先删除再导入", manifest.name)
        } else {
            error.to_string()
        };
        let rollback = ctx.update("skills", json!({}), |saved| {
            let object = saved
                .as_object_mut()
                .ok_or_else(|| "skills.json 格式错误".to_string())?;
            if let Some(value) = previous.get(&manifest.name) {
                object.insert(manifest.name.clone(), json!(value));
            } else {
                object.remove(&manifest.name);
            }
            Ok(())
        });
        if let Some(path) = reserved {
            let _ = fs::remove_dir(path);
        }
        return Err(match rollback {
            Ok(_) => message,
            Err(rollback) => format!("{message}；恢复 Skill 状态失败：{rollback}"),
        });
    }
    Ok(json!(SkillInfo {
        enabled: true,
        name: manifest.name,
        description: manifest.description,
    }))
}

fn installed_dir(ctx: &Ctx, name: &str) -> Result<PathBuf, String> {
    let root = skills_root(ctx)?;
    installed_at(&root, name).map(|(dir, _)| dir)
}

fn set_enabled(ctx: &Ctx, name: &str, enabled: bool) -> Reply {
    let _guard = skill_guard()?;
    installed_dir(ctx, name)?;
    update_state(ctx, |saved| {
        saved.insert(name.to_owned(), json!(enabled));
        Ok(())
    })?;
    Ok(Value::Null)
}

fn delete_skill(ctx: &Ctx, name: &str) -> Reply {
    let _guard = skill_guard()?;
    let dir = installed_dir(ctx, name)?;
    state(ctx)?;
    fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    update_state(ctx, |saved| {
        saved.remove(name);
        Ok(())
    })?;
    Ok(Value::Null)
}

fn read_skill(ctx: &Ctx, name: &str, raw_path: &str, enabled_only: bool) -> Reply {
    let _guard = skill_guard()?;
    let dir = installed_dir(ctx, name)?;
    if enabled_only && !is_enabled(&state(ctx)?, name) {
        return Err(format!("Skill {name} 已停用"));
    }
    let relative = relative_path(raw_path)?;
    let mut target = dir.clone();
    let parts = relative
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        target.push(part);
        let meta = fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            return Err("Skill 路径不能包含符号链接".into());
        }
        if index + 1 < parts.len() && !meta.is_dir() {
            return Err("Skill 路径无效".into());
        }
    }
    Ok(json!({
        "name": name,
        "path": relative.to_string_lossy(),
        "content": read_text_within(&target, &dir, MAX_READ)?,
    }))
}

pub fn handle(cmd: &str, a: &Value, ctx: &Ctx, _app: &tauri::AppHandle) -> Option<Reply> {
    (|| -> Result<Option<Value>, String> {
        Ok(Some(match cmd {
            "pick_skill_file" => json!(rfd::FileDialog::new()
                .add_filter("Skill", &["skill", "zip", "md"])
                .pick_file()),
            "list_skills" => list_skills(ctx)?,
            "import_skill" => import_skill(ctx, Path::new(arg(a, "path")?))?,
            "set_skill_enabled" => {
                let enabled = a["enabled"].as_bool().ok_or("缺少布尔参数 enabled")?;
                set_enabled(ctx, arg(a, "name")?, enabled)?
            }
            "delete_skill" => delete_skill(ctx, arg(a, "name")?)?,
            "read_skill" => {
                let path = a["path"].as_str().unwrap_or("SKILL.md");
                read_skill(
                    ctx,
                    arg(a, "name")?,
                    path,
                    a["enabledOnly"].as_bool().unwrap_or(false),
                )?
            }
            _ => return Ok(None),
        }))
    })()
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# Instructions\n\nUse this skill.\n")
    }

    fn write_zip(path: &Path, entries: &[(&str, &str, Option<u32>)]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, content, _) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        if entries.iter().any(|(_, _, mode)| mode.is_some()) {
            let mut data = fs::read(path).unwrap();
            let eocd = data.len() - 22;
            let count = u16::from_le_bytes([data[eocd + 10], data[eocd + 11]]) as usize;
            let mut offset =
                u32::from_le_bytes(data[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
            for (index, (_, _, mode)) in entries.iter().enumerate() {
                assert_eq!(&data[offset..offset + 4], &[0x50, 0x4b, 0x01, 0x02]);
                if let Some(mode) = mode {
                    data[offset + 38..offset + 42].copy_from_slice(&((*mode) << 16).to_le_bytes());
                }
                if index + 1 < count {
                    let name_len =
                        u16::from_le_bytes(data[offset + 28..offset + 30].try_into().unwrap())
                            as usize;
                    let extra_len =
                        u16::from_le_bytes(data[offset + 30..offset + 32].try_into().unwrap())
                            as usize;
                    let comment_len =
                        u16::from_le_bytes(data[offset + 32..offset + 34].try_into().unwrap())
                            as usize;
                    offset += 46 + name_len + extra_len + comment_len;
                }
            }
            fs::write(path, data).unwrap();
        }
    }

    #[test]
    fn validates_manifest_metadata() {
        assert!(parse_manifest(&manifest("ono-dev", "Develop ono")).is_ok());
        assert!(parse_manifest(&manifest("Ono_Dev", "Develop ono")).is_err());
        assert!(parse_manifest(&manifest("ono--dev", "Develop ono")).is_err());
        assert!(parse_manifest(&manifest("con", "Develop ono")).is_err());
        assert!(parse_manifest("---\nname: ono-dev\ndescription: ok\n---\n").is_err());
        assert!(parse_manifest(&manifest("ono-dev", "line one\nline two")).is_err());
    }

    #[test]
    fn rejects_excessive_zip_entry_count_before_parsing() {
        let mut data = vec![0_u8; 22];
        data[..4].copy_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        data[8..10].copy_from_slice(&(MAX_ENTRIES as u16 + 1).to_le_bytes());
        data[10..12].copy_from_slice(&(MAX_ENTRIES as u16 + 1).to_le_bytes());
        assert!(preflight_zip(&data).unwrap_err().contains("文件过多"));
    }

    #[test]
    fn rejects_zip_directory_expansion() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("deep.zip");
        let deep_path = std::iter::repeat("d")
            .take(MAX_ENTRIES + 1)
            .collect::<Vec<_>>()
            .join("/");
        write_zip(&archive, &[(&deep_path, "", None)]);
        let dest = temp.path().join("unpacked");
        fs::create_dir(&dest).unwrap();

        assert!(extract_zip(fs::read(&archive).unwrap(), &dest)
            .unwrap_err()
            .contains("文件夹过多"));
        let mut manifests = Vec::new();
        let mut visited = 0;
        find_manifests(&dest, &mut manifests, &mut visited).unwrap();
        assert_eq!(visited, MAX_ENTRIES);
    }

    #[test]
    fn rejects_malformed_state_without_poisoning_context() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(temp.path().join("data")).unwrap();
        let source = temp.path().join("SKILL.md");
        fs::write(&source, manifest("ono-dev", "Develop ono")).unwrap();
        ctx.write("skills", &json!({"ono-dev": "yes"})).unwrap();

        assert!(import_skill(&ctx, &source).unwrap_err().contains("布尔值"));
        assert!(!ctx.root.join("skills").join("ono-dev").exists());
        assert!(ctx.read("settings", json!({})).is_ok());

        ctx.write("skills", &json!({})).unwrap();
        assert_eq!(list_skills(&ctx).unwrap(), json!([]));
        import_skill(&ctx, &source).unwrap();
        ctx.write("skills", &json!({"ono-dev": "yes"})).unwrap();
        assert!(delete_skill(&ctx, "ono-dev").is_err());
        assert!(ctx.root.join("skills").join("ono-dev").is_dir());
        assert!(ctx.read("settings", json!({})).is_ok());
        ctx.write("skills", &json!({})).unwrap();
        delete_skill(&ctx, "ono-dev").unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_linked_skills_root() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(temp.path().join("data")).unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, ctx.root.join("skills")).unwrap();
        assert!(skills_root(&ctx).unwrap_err().contains("无效"));
    }

    #[test]
    fn imports_lists_toggles_reads_and_deletes_markdown() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(temp.path().join("data")).unwrap();
        let source = temp.path().join("SKILL.md");
        fs::write(&source, manifest("ono-dev", "Develop ono safely")).unwrap();

        let imported = import_skill(&ctx, &source).unwrap();
        assert_eq!(imported["name"], "ono-dev");
        assert_eq!(imported["enabled"], true);
        assert_eq!(
            list_skills(&ctx).unwrap()[0]["description"],
            "Develop ono safely"
        );
        assert!(import_skill(&ctx, &source).is_err());

        set_enabled(&ctx, "ono-dev", false).unwrap();
        assert_eq!(list_skills(&ctx).unwrap()[0]["enabled"], false);
        assert!(read_skill(&ctx, "ono-dev", "SKILL.md", true).is_err());
        assert!(read_skill(&ctx, "ono-dev", "../settings.json", false).is_err());
        assert!(read_skill(&ctx, "ono-dev", "SKILL.md", false).is_ok());

        delete_skill(&ctx, "ono-dev").unwrap();
        assert_eq!(list_skills(&ctx).unwrap(), json!([]));
    }

    #[test]
    fn imports_wrapped_zip_with_resources() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(temp.path().join("data")).unwrap();
        let source = temp.path().join("ono-dev.skill");
        write_zip(
            &source,
            &[
                (
                    "wrapper/SKILL.md",
                    &manifest("ono-dev", "Develop ono"),
                    None,
                ),
                ("wrapper/references/ipc.md", "Dynamic dispatch", None),
            ],
        );

        assert_eq!(import_skill(&ctx, &source).unwrap()["name"], "ono-dev");
        let read = read_skill(&ctx, "ono-dev", "references/ipc.md", true).unwrap();
        assert_eq!(read["content"], "Dynamic dispatch");
    }

    #[test]
    fn rejects_unsafe_archives() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(temp.path().join("data")).unwrap();
        let traversal = temp.path().join("traversal.zip");
        write_zip(
            &traversal,
            &[("../SKILL.md", &manifest("ono-dev", "Develop ono"), None)],
        );
        assert!(import_skill(&ctx, &traversal).is_err());

        let symlink = temp.path().join("symlink.zip");
        write_zip(
            &symlink,
            &[
                ("SKILL.md", &manifest("ono-dev", "Develop ono"), None),
                ("references", "outside", Some(0o120777)),
            ],
        );
        assert!(import_skill(&ctx, &symlink).is_err());
        assert_eq!(list_skills(&ctx).unwrap(), json!([]));
    }
}
