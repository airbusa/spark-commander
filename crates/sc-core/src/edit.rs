//! Text files for the editor: binary or other text shown only on request, as editing could change them by accident.

pub mod buffer;
pub mod syntax;
pub mod text_edit;
pub mod text_find;

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// What the editor gets.
#[derive(Debug, PartialEq)]
pub enum Loaded {
    Text(String),
    /// Editing it as text would change it: why (`binary`, `not_utf8`), and the text shown anyway on request.
    Unfit { why: &'static str, anyway: String },
}

/// Up to `limit_mb`, as Settings' Edit/View page says.
pub fn load_text(path: &Path, limit_mb: u64) -> io::Result<Loaded> {
    let refuse = |why: &str| Err(io::Error::new(io::ErrorKind::InvalidData, why));
    if fs::metadata(path)?.len() > limit_mb << 20 {
        let why = crate::t!("editor.open.too_large", limit_mb = limit_mb);
        return refuse(&why);
    }
    let bytes = fs::read(path)?;
    if bytes.contains(&0) {
        let anyway = as_text_anyway(&bytes);
        return Ok(Loaded::Unfit { why: "binary", anyway });
    }
    Ok(match String::from_utf8(bytes) {
        Ok(text) => Loaded::Text(text),
        Err(err) => Loaded::Unfit {
            why: "not_utf8",
            anyway: as_text_anyway(err.as_bytes()),
        },
    })
}

/// Bytes as text anyway, as VS Code shows them: what is not UTF-8 as `�`,
/// control characters as their symbols (`␀`).
pub fn as_text_anyway(bytes: &[u8]) -> String {
    let shown = String::from_utf8_lossy(bytes);
    let symbol = |c: char| match c {
        '\t' | '\n' | '\r' => c,
        '\0'..='\x1f' => char::from_u32(0x2400 + c as u32).unwrap_or(c),
        '\x7f' => '\u{2421}',
        c => c,
    };
    shown.chars().map(symbol).collect()
}

/// Written beside the file and renamed over it, with its permissions; a
/// link's target is what gets saved; a new file is written as it is.
pub fn save_text(path: &Path, text: &str) -> io::Result<()> {
    let target = match fs::canonicalize(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => return fs::write(path, text),
        found => found?,
    };
    let permissions = fs::metadata(&target)?.permissions();
    let temp = target.with_file_name(save_name(&target));
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .and_then(|mut file| io::Write::write_all(&mut file, text.as_bytes()))
        .and_then(|()| fs::set_permissions(&temp, permissions))
        .and_then(|()| fs::rename(&temp, &target));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// A save's own temporary name, beside another editor's saving the same file.
fn save_name(path: &Path) -> std::ffi::OsString {
    static SAVES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let save = SAVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(format!(".{}-{save}.sc-save", std::process::id()));
    name
}

/// `text` in a new file only the user may read, to be written into `path`
/// as administrator (`elevate::Action::Write`).
pub fn stage_text(path: &Path, text: &str) -> io::Result<std::path::PathBuf> {
    use std::io::Write as _;
    let staged = std::env::temp_dir().join(save_name(path));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(&staged)?.write_all(text.as_bytes())?;
    Ok(staged)
}

/// Starts the external editor on `file`, as Ctrl+Shift+F4 does.
pub fn edit_externally(line: &str, file: &Path) -> io::Result<()> {
    let mut child = start_editor(line, file)?;
    // Reaps it once it ends.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The external editor on `file`; waiting for it tells when editing ends,
/// if the editor waits too (`code --wait`).
pub fn start_editor(line: &str, file: &Path) -> io::Result<Child> {
    let (program, args) = command_line(line, file)
        .ok_or_else(|| io::Error::other(crate::t!("editor.external.not_set")))?;
    Command::new(&program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| io::Error::new(err.kind(), format!("{}: {err}", program.to_string_lossy())))
}

/// Editors for when neither `$VISUAL` nor `$EDITOR` is set.
const TERMINAL_EDITORS: &[&str] = if cfg!(windows) {
    &["notepad"]
} else {
    &["nano", "vi"]
};

/// F4 in the terminal: `$VISUAL`, `$EDITOR`, else nano or vi, in this
/// terminal until it ends.
pub fn edit_in_terminal(file: &Path) -> io::Result<()> {
    let set = ["VISUAL", "EDITOR"].iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|line| !line.trim().is_empty())
    });
    let lines = match &set {
        Some(line) => vec![line.as_str()],
        None => TERMINAL_EDITORS.to_vec(),
    };
    for line in lines {
        let Some((program, args)) = command_line(line, file) else {
            continue;
        };
        match Command::new(&program).args(args).status() {
            Err(err) if err.kind() == io::ErrorKind::NotFound && set.is_none() => continue,
            Err(err) => {
                let program = program.to_string_lossy();
                return Err(io::Error::new(err.kind(), format!("{program}: {err}")));
            }
            Ok(_) => return Ok(()),
        }
    }
    let why = crate::t!("editor.terminal.not_found");
    Err(io::Error::new(io::ErrorKind::NotFound, why))
}

/// Words of `line`, `"quoted"` ones with their spaces; `%1` becomes `file`,
/// which goes last when no word has it.
fn command_line(line: &str, file: &Path) -> Option<(OsString, Vec<OsString>)> {
    let mut words = Vec::new();
    let (mut word, mut quoted, mut started) = (String::new(), false, false);
    for c in line.chars() {
        match c {
            '"' => (quoted, started) = (!quoted, true),
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                }
                started = false;
            }
            c => (word, started) = (word + &c.to_string(), true),
        }
    }
    if started {
        words.push(word);
    }
    let mut placed = false;
    let mut args: Vec<OsString> = words
        .iter()
        .map(|word| {
            let mut arg = OsString::new();
            for (i, part) in word.split("%1").enumerate() {
                if i > 0 {
                    arg.push(file);
                    placed = true;
                }
                arg.push(part);
            }
            arg
        })
        .collect();
    if !placed {
        args.push(file.into());
    }
    let program = (!words.is_empty()).then(|| args.remove(0))?;
    Some((program, args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_external_editors_command() {
        let file = Path::new("/tmp/a b.txt");
        let run = |line: &str| {
            let (program, args) = command_line(line, file).unwrap();
            let mut all = vec![program];
            all.extend(args);
            all.into_iter()
                .map(|arg| arg.into_string().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(run("xdg-open"), ["xdg-open", "/tmp/a b.txt"]);
        assert_eq!(run("code  --wait %1"), ["code", "--wait", "/tmp/a b.txt"]);
        assert_eq!(
            run(r#""C:\Program Files\Ed\ed.exe" --file="%1" -n"#),
            [r"C:\Program Files\Ed\ed.exe", "--file=/tmp/a b.txt", "-n"]
        );
        assert!(command_line("  ", file).is_none());
    }

    #[test]
    fn loads_and_saves_text() {
        let dir = std::env::temp_dir().join(format!("sc-edit-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        fs::write(&file, "one\r\ntwo").unwrap();
        assert_eq!(
            load_text(&file, 32).unwrap(),
            Loaded::Text("one\r\ntwo".into()),
            "line ends kept"
        );
        save_text(&file, "three\r\n").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "three\r\n");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "no temporary left");
        // Save As: a new file.
        let new = file.with_file_name("new.txt");
        save_text(&new, "four").unwrap();
        assert_eq!(fs::read_to_string(&new).unwrap(), "four");

        fs::write(dir.join("bin"), b"a\0b\x7f\tc").unwrap();
        let binary = Loaded::Unfit {
            why: "binary",
            anyway: "a\u{2400}b\u{2421}\tc".into(),
        };
        assert_eq!(load_text(&dir.join("bin"), 32).unwrap(), binary);
        fs::write(dir.join("latin1"), b"caf\xe9").unwrap();
        let latin = Loaded::Unfit {
            why: "not_utf8",
            anyway: "caf\u{fffd}".into(),
        };
        assert_eq!(load_text(&dir.join("latin1"), 32).unwrap(), latin);
        assert!(load_text(&file, 0).is_err(), "over the limit");
        fs::remove_dir_all(&dir).unwrap();
    }
}
