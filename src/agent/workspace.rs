use super::{arg, err, files, Reply};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

// Resolve symlinks before processing "..", including when only a path prefix exists.
fn physical(path: &Path) -> Result<PathBuf, String> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            // A Windows verbatim prefix like \\?\D: is not a complete path.
            Component::Prefix(_) => out.push(part.as_os_str()),
            _ => {
                out.push(part.as_os_str());
                match fs::symlink_metadata(&out) {
                    Ok(m) if m.file_type().is_symlink() => out = out.canonicalize().map_err(err)?,
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(err(e)),
                    _ => {}
                }
            }
        }
    }
    // Normalize the Windows verbatim prefix for existing and new paths alike.
    if out.exists() {
        return out.canonicalize().map_err(err);
    }
    Ok(out)
}
pub(super) fn resolve(a: &Value) -> Reply {
    let root = Path::new(arg(a, "ws")?).canonicalize().map_err(err)?;
    let path = physical(&root.join(arg(a, "path")?))?;
    Ok(json!({"path":path,"inside":path.starts_with(&root),"exists":path.exists()}))
}

fn walk(
    root: &Path,
    depth: u64,
    visit: &mut impl FnMut(&Path, &fs::FileType) -> Result<(), String>,
    budget: &mut usize,
) -> Result<(), String> {
    if depth == 0 || *budget == 0 {
        return Ok(());
    }
    let mut entries = fs::read_dir(root)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if *budget == 0 {
            break;
        }
        let path = entry.path();
        let ty = entry.file_type().map_err(err)?;
        if ty.is_dir()
            && matches!(
                entry.file_name().to_str(),
                Some(".git" | "node_modules" | "target" | ".venv" | "__pycache__")
            )
        {
            continue;
        }
        *budget -= 1;
        visit(&path, &ty)?;
        if ty.is_dir() {
            walk(&path, depth - 1, visit, budget)?;
        }
    }
    Ok(())
}
struct ShellProcess {
    child: Child,
    task_id: String,
}

// None prevents queued IPC workers from spawning a process during shutdown.
static SHELLS: Mutex<Option<Vec<ShellProcess>>> = Mutex::new(Some(Vec::new()));
fn terminate(child: &mut Child) {
    #[cfg(unix)]
    let _ = Command::new("/bin/kill")
        .args(["-KILL", &format!("-{}", child.id())])
        .output();
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        crate::hide_console_window(&mut command);
        let _ = command
            .args(["/F", "/T", "/PID", &child.id().to_string()])
            .output();
    }
    let _ = child.kill();
    let _ = child.wait();
}
pub fn shutdown() {
    if let Some(mut children) = SHELLS.lock().unwrap().take() {
        for process in &mut children {
            terminate(&mut process.child);
        }
    }
}
pub fn abort(task_id: &str) -> Reply {
    let mut stopped = 0;
    let mut guard = SHELLS.lock().map_err(err)?;
    if let Some(children) = guard.as_mut() {
        let mut index = 0;
        while index < children.len() {
            if children[index].task_id == task_id {
                terminate(&mut children.remove(index).child);
                stopped += 1;
            } else {
                index += 1;
            }
        }
    }
    Ok(json!(stopped))
}
fn supervise(root: &Path, mut cmd: Command, a: &Value) -> Reply {
    let mut log = tempfile::tempfile().map_err(err)?;
    let task_id = a["taskId"].as_str().unwrap_or("").to_owned();
    let id = {
        let mut guard = SHELLS.lock().map_err(err)?;
        let children = guard.as_mut().ok_or("应用正在退出")?;
        let child = cmd
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(err)?)
            .stderr(log.try_clone().map_err(err)?)
            .spawn()
            .map_err(err)?;
        let id = child.id();
        children.push(ShellProcess { child, task_id });
        id
    };
    let until =
        Instant::now() + Duration::from_secs(a["timeout"].as_u64().unwrap_or(120).clamp(1, 600));
    let status = loop {
        {
            let mut guard = SHELLS.lock().map_err(err)?;
            let children = guard.as_mut().ok_or("应用正在退出")?;
            let pos = children
                .iter()
                .position(|process| process.child.id() == id)
                .ok_or("命令已停止")?;
            match children[pos].child.try_wait() {
                Ok(Some(status)) => {
                    children.swap_remove(pos);
                    break status.to_string();
                }
                Ok(None) if Instant::now() >= until => {
                    terminate(&mut children.swap_remove(pos).child);
                    break "命令超时".into();
                }
                Err(e) => {
                    terminate(&mut children.swap_remove(pos).child);
                    return Err(err(e));
                }
                _ => {}
            }
        }
        thread::sleep(Duration::from_millis(40));
    };
    log.seek(SeekFrom::Start(0)).map_err(err)?;
    let mut bytes = Vec::new();
    log.take(65536).read_to_end(&mut bytes).map_err(err)?;
    Ok(json!(format!(
        "{status}\n{}",
        String::from_utf8_lossy(&bytes)
    )))
}
fn shell(root: &Path, a: &Value) -> Reply {
    let command = arg(a, "command")?;
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("powershell.exe");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); {command}"),
        ]);
        c
    };
    #[cfg(unix)]
    let mut cmd = {
        use std::os::unix::process::CommandExt;
        let mut c = Command::new("sh");
        c.args(["-c", command]).process_group(0);
        c
    };
    crate::hide_console_window(&mut cmd);
    supervise(root, cmd, a)
}
fn code(root: &Path, a: &Value) -> Reply {
    let language = arg(a, "language")?.trim().to_ascii_lowercase();
    let source = arg(a, "code")?;
    if source.chars().count() > 262_144 {
        return Err("代码不能超过 262144 字符".into());
    }
    let mut cmd = match language.as_str() {
        "python" => {
            #[cfg(windows)]
            let mut cmd = {
                let mut cmd = Command::new("py");
                cmd.args(["-3", "-u"]);
                cmd
            };
            #[cfg(not(windows))]
            let mut cmd = Command::new("python3");
            cmd.args(["-c", source]);
            cmd
        }
        "javascript" | "node" => {
            let mut cmd = Command::new("node");
            cmd.args(["-e", source]);
            cmd
        }
        "powershell" => {
            let mut cmd = Command::new("powershell.exe");
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", source]);
            cmd
        }
        "bash" | "sh" => {
            let mut cmd = Command::new("sh");
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                cmd.process_group(0);
            }
            cmd.args(["-c", source]);
            cmd
        }
        _ => return Err("仅支持 python、javascript、powershell 或 bash".into()),
    };
    crate::hide_console_window(&mut cmd);
    supervise(root, cmd, a)
}

const MAX_PACKAGE_FILE: u64 = 32 * 1024 * 1024;
const MAX_PACKAGE_TOTAL: u64 = 128 * 1024 * 1024;
const MAX_PACKAGE_ENTRIES: usize = 1000;

fn package(root: &Path, a: &Value) -> Reply {
    let raw = root.join(arg(a, "path")?);
    if fs::symlink_metadata(&raw)
        .map_err(err)?
        .file_type()
        .is_symlink()
    {
        return Err("不能打包符号链接".into());
    }
    let path = physical(&raw)?;
    if !path.starts_with(root) {
        return Err("只能打包工作文件夹里的内容".into());
    }
    let output_raw = root.join(arg(a, "output_path")?);
    if output_raw
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err("打包目标不能是符号链接".into());
    }
    let output = physical(&output_raw)?;
    if !output.starts_with(root) {
        return Err("打包文件必须保存在工作文件夹里".into());
    }
    if output == path || (path.is_dir() && output.starts_with(&path)) {
        return Err("打包文件不能放在被打包的文件夹里面".into());
    }
    if output.is_dir() {
        return Err("打包目标必须是文件".into());
    }
    let parent = output.parent().ok_or("打包目标没有上级文件夹")?;
    fs::create_dir_all(parent).map_err(err)?;
    let mut files = Vec::new();
    let mut total = 0_u64;
    let mut budget = MAX_PACKAGE_ENTRIES;
    let mut visit = |file: &Path, ty: &fs::FileType| -> Result<(), String> {
        if ty.is_symlink() {
            return Err("打包内容不能包含符号链接".into());
        }
        if !ty.is_file() {
            return Ok(());
        }
        let size = fs::metadata(file).map_err(err)?.len();
        if size > MAX_PACKAGE_FILE {
            return Err("打包中的单个文件超过 32 MiB".into());
        }
        total = total.checked_add(size).ok_or("打包内容过大")?;
        if total > MAX_PACKAGE_TOTAL {
            return Err("打包内容超过 128 MiB".into());
        }
        let relative = file.strip_prefix(root).map_err(err)?;
        files.push((
            file.to_path_buf(),
            relative.to_string_lossy().replace('\\', "/"),
        ));
        Ok(())
    };
    if path.is_file() {
        visit(&path, &fs::metadata(&path).map_err(err)?.file_type())?;
    } else {
        walk(&path, 128, &mut visit, &mut budget)?;
    }
    if files.len() > MAX_PACKAGE_ENTRIES {
        return Err("打包内容超过 1000 个文件".into());
    }
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
    {
        let mut archive = ZipWriter::new(temp.as_file_mut());
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (file, name) in files {
            let input = fs::File::open(file).map_err(err)?;
            archive.start_file(name, options.clone()).map_err(err)?;
            let copied =
                std::io::copy(&mut input.take(MAX_PACKAGE_FILE + 1), &mut archive).map_err(err)?;
            if copied > MAX_PACKAGE_FILE {
                return Err("打包中的单个文件超过 32 MiB".into());
            }
        }
        archive.finish().map_err(err)?;
    }
    temp.flush().map_err(err)?;
    temp.as_file().sync_all().map_err(err)?;
    temp.persist(&output).map_err(err)?;
    Ok(json!(format!("已打包 {}", output.display())))
}

pub(super) fn run(ws: &Path, name: &str, a: &Value) -> Reply {
    let root = ws.canonicalize().map_err(err)?;
    if name == "run_command" {
        return shell(&root, a);
    }
    if name == "run_code" {
        return code(&root, a);
    }
    let raw = root.join(a["path"].as_str().unwrap_or("."));
    let path = physical(&raw)?;
    match name {
        "package_files" => package(&root, a),
        "list_files" | "search_files" => {
            let search = name == "search_files";
            let depth = if search {
                128
            } else {
                a["depth"].as_u64().unwrap_or(3).clamp(1, 6)
            };
            let pattern = if search {
                arg(a, "pattern")?.to_lowercase()
            } else {
                String::new()
            };
            let types = a["file_types"]
                .as_str()
                .unwrap_or("")
                .split(',')
                .map(|s| s.trim().trim_start_matches('.').to_lowercase())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            let mut lines = Vec::new();
            let mut visit = |p: &Path, ty: &fs::FileType| {
                if lines.len() >= 2000 {
                    return Ok(());
                }
                let rel = p.strip_prefix(&path).unwrap_or(p).display().to_string();
                if !search {
                    lines.push(format!("{rel}{}", if ty.is_dir() { "/" } else { "" }));
                    return Ok(());
                }
                if !types.is_empty()
                    && !types.contains(
                        &p.extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_lowercase(),
                    )
                {
                    return Ok(());
                }
                if rel.to_lowercase().contains(&pattern) {
                    lines.push(rel.clone());
                }
                if ty.is_file() && p.metadata().map(|m| m.len() <= 2_000_000).unwrap_or(false) {
                    if let Ok(s) = fs::read_to_string(p) {
                        for (n, l) in s.lines().enumerate() {
                            if lines.len() >= 2000 {
                                break;
                            }
                            if l.to_lowercase().contains(&pattern) {
                                lines.push(format!(
                                    "{rel}:{}: {}",
                                    n + 1,
                                    l.chars().take(500).collect::<String>()
                                ));
                            }
                        }
                    }
                }
                Ok(())
            };
            if path.is_file() {
                visit(&path, &path.metadata().map_err(err)?.file_type())?;
            } else {
                walk(&path, depth, &mut visit, &mut 20000)?;
            }
            Ok(json!(lines.join("\n")))
        }
        "read_file" => {
            let start = a["start_line"].as_u64().unwrap_or(1).max(1) as usize;
            let count = a["max_lines"].as_u64().unwrap_or(2000).clamp(1, 20000) as usize;
            let data = files::document(&path)?;
            Ok(json!(data
                .lines()
                .enumerate()
                .skip(start - 1)
                .take(count)
                .map(|(i, l)| format!("{}\t{l}", i + 1))
                .collect::<Vec<_>>()
                .join("\n")))
        }
        "write_file" => {
            let s = arg(a, "content")?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(err)?;
            }
            fs::write(&path, s).map_err(err)?;
            Ok(json!(format!("已写入 {}", path.display())))
        }
        "edit_file" => {
            let s = fs::read_to_string(&path).map_err(err)?;
            let old = arg(a, "old_text")?;
            let new = arg(a, "new_text")?;
            if old.is_empty() || !s.contains(old) {
                return Err("未找到原文".into());
            }
            if !a["replace_all"].as_bool().unwrap_or(false) && s.matches(old).count() != 1 {
                return Err("原文出现多次，请增加上下文".into());
            }
            fs::write(&path, s.replace(old, new)).map_err(err)?;
            Ok(json!(format!("已修改 {}", path.display())))
        }
        "make_dir" => {
            fs::create_dir_all(&path).map_err(err)?;
            Ok(json!(format!("已创建 {}", path.display())))
        }
        "delete_path" => {
            if path == root || path.parent().is_none() {
                return Err("不能删除工作文件夹或文件系统根目录".into());
            }
            let ty = fs::symlink_metadata(&raw).map_err(err)?.file_type();
            if ty.is_dir() {
                fs::remove_dir_all(&raw)
            } else {
                fs::remove_file(&raw)
            }
            .map_err(err)?;
            Ok(json!(format!("已删除 {}", raw.display())))
        }
        _ => Err(format!("未知工具 {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_workspace_dot_resolves() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        assert_eq!(physical(&root.join(".")).unwrap(), root);
    }

    #[test]
    fn canonical_workspace_writes_and_packages_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        run(
            &root,
            "write_file",
            &json!({"path":"src/a.txt","content":"alpha"}),
        )
        .unwrap();
        assert!(run(
            &root,
            "write_file",
            &json!({"path":"src","content":"not a file"})
        )
        .is_err());
        run(
            &root,
            "package_files",
            &json!({"path":".","output_path":"delivery.zip"}),
        )
        .unwrap();
        let archive =
            zip::ZipArchive::new(fs::File::open(root.join("delivery.zip")).unwrap()).unwrap();
        assert!(archive.file_names().any(|name| name == "src/a.txt"));
    }

    #[test]
    fn edits_require_unique_matches_and_preserve_other_content() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        run(
            root,
            "write_file",
            &json!({"path":"dir/a.txt","content":"x\nx\ny"}),
        )
        .unwrap();
        let mut args = json!({"path":"dir/a.txt","old_text":"x","new_text":"z"});
        assert!(run(root, "edit_file", &args).is_err());
        args["replace_all"] = json!(true);
        run(root, "edit_file", &args).unwrap();
        assert_eq!(
            run(
                root,
                "read_file",
                &json!({"path":"dir/a.txt","start_line":2})
            )
            .unwrap(),
            "2\tz\n3\ty"
        );
    }

    #[test]
    fn packages_workspace_files_and_rejects_self_inclusion() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        run(
            root,
            "write_file",
            &json!({"path":"src/a.txt","content":"alpha"}),
        )
        .unwrap();
        run(
            root,
            "write_file",
            &json!({"path":"README.md","content":"ono"}),
        )
        .unwrap();
        run(
            root,
            "package_files",
            &json!({"path":".","output_path":"delivery.zip"}),
        )
        .unwrap();
        let archive =
            zip::ZipArchive::new(fs::File::open(root.join("delivery.zip")).unwrap()).unwrap();
        let names = archive.file_names().collect::<Vec<_>>();
        assert!(names.contains(&"src/a.txt"));
        assert!(names.contains(&"README.md"));
        assert!(!names.contains(&"delivery.zip"));
        assert!(run(
            root,
            "package_files",
            &json!({"path":"src","output_path":"src/a.zip"})
        )
        .is_err());
    }

    #[test]
    fn rejects_unknown_code_runtime() {
        let temp = tempfile::tempdir().unwrap();
        assert!(run(
            temp.path(),
            "run_code",
            &json!({"language":"ruby","code":"puts 1"})
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn new_files_through_external_symlinks_are_outside() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("ws");
        fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(temp.path(), root.join("link")).unwrap();
        for path in ["../new", "link/new", "link/../new"] {
            assert_eq!(
                resolve(&json!({"ws":root,"path":path})).unwrap()["inside"],
                false
            );
        }
        assert_eq!(
            resolve(&json!({"ws":root,"path":"new/child"})).unwrap()["inside"],
            true
        );
    }
}
