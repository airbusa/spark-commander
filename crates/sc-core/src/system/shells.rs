//! The shells installed, for the terminal to start one of instead of the system's own.

#[cfg(unix)]
use std::path::Path;

/// A shell: its name in the settings and its command line.
#[derive(Clone, Debug, PartialEq)]
pub struct Shell {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

/// The shell the terminal starts when none is chosen: the user's own (`$SHELL`), Windows PowerShell on Windows.
pub fn default_name() -> String {
    match cfg!(windows) {
        true => "Windows PowerShell".into(),
        false => std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()),
    }
}

/// The shells installed besides the default one.
pub fn others() -> Vec<Shell> {
    #[cfg(unix)]
    return from_list(&std::fs::read_to_string("/etc/shells").unwrap_or_default(), &default_name());
    #[cfg(windows)]
    return windows();
}

/// The command line of the `chosen` shell; none for the default, or one gone since.
pub fn command(chosen: Option<&str>) -> Option<(String, Vec<String>)> {
    let chosen = chosen?;
    let shell = others().into_iter().find(|shell| shell.program == chosen)?;
    // The terminal puts it unquoted into Windows' command line: "C:\Program Files\…" would run C:\Program.
    let program = if cfg!(windows) { format!("\"{}\"", shell.program) } else { shell.program };
    Some((program, shell.args))
}

/// `/etc/shells`' programs there, each once (`/bin` is `/usr/bin` on most systems), but `default`.
#[cfg(unix)]
fn from_list(list: &str, default: &str) -> Vec<Shell> {
    // Listed there, but no shells to work in.
    const NOT: [&str; 7] = ["tmux", "screen", "nologin", "false", "true", "git-shell", "rbash"];
    let real = |path: &Path| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?));
    let default = real(Path::new(default));
    let mut shells: Vec<Shell> = Vec::new();
    for line in list.lines().map(str::trim).filter(|line| line.starts_with('/')) {
        let Some(program) = real(Path::new(line)).filter(|program| program.is_file()) else {
            continue;
        };
        let name = program.to_string_lossy().into_owned();
        let tool = program.file_name().is_some_and(|file| NOT.iter().any(|not| file == *not));
        if tool || Some(&program) == default.as_ref() || shells.iter().any(|shell| shell.name == name) {
            continue;
        }
        // macOS's terminals start login shells, as the default one is.
        let args = if cfg!(target_os = "macos") { vec!["-l".into()] } else { Vec::new() };
        shells.push(Shell { name: name.clone(), program: name, args });
    }
    shells
}

#[cfg(windows)]
fn windows() -> Vec<Shell> {
    use std::path::{Path, PathBuf};
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    // A Store app's alias is a link `is_file` can't follow.
    let there = |path: &Path| path.symlink_metadata().is_ok();
    let system = env("SystemRoot").unwrap_or_else(|| r"C:\Windows".into()).join("System32");
    let programs = env("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into());
    let in_path = |exe: &str| std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(exe)).find(|path| there(path));
    let pwsh = Some(programs.join(r"PowerShell\7\pwsh.exe")).filter(|path| there(path)).or_else(|| in_path("pwsh.exe"));
    let git = [Some(programs.join(r"Git\bin\bash.exe")), env("LOCALAPPDATA").map(|dir| dir.join(r"Programs\Git\bin\bash.exe"))];
    let git = git.into_iter().flatten().find(|path| there(path));
    // wsl.exe is there also with no Linux installed, which it only offers to install.
    let wsl = unsafe { RegGetValueW(HKEY_CURRENT_USER, w!(r"Software\Microsoft\Windows\CurrentVersion\Lxss"), w!("DefaultDistribution"), RRF_RT_REG_SZ, None, None, None) }.is_ok();
    let prompt = crate::t!("shells.command_prompt");
    let found = [
        ("PowerShell 7", pwsh, &[][..]),
        (prompt.as_str(), Some(system.join("cmd.exe")), &[]),
        ("Git Bash", git, &["-i", "-l"]),
        ("WSL", wsl.then(|| system.join("wsl.exe")), &[]),
    ];
    found
        .into_iter()
        .filter_map(|(name, program, args)| {
            let program = program.filter(|path| there(path))?;
            Some(Shell { name: name.into(), program: program.to_string_lossy().into_owned(), args: args.iter().map(|arg| arg.to_string()).collect() })
        })
        .collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn lists_each_shell_once_without_the_default_or_tools() {
        let root = std::env::temp_dir().join(format!("sc-shells-{}", std::process::id()));
        let bin = root.join("usr/bin");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["bash", "zsh", "tmux"] {
            std::fs::write(bin.join(name), "").unwrap();
        }
        std::os::unix::fs::symlink(&bin, root.join("bin")).unwrap();
        let r = root.display();
        let list = format!("# shells\n{r}/bin/bash\n{r}/bin/zsh\n{r}/usr/bin/zsh\n{r}/usr/bin/tmux\n{r}/usr/bin/fish\n");
        let shells = from_list(&list, &format!("{r}/bin/bash"));
        let zsh = bin.canonicalize().unwrap().join("zsh").to_string_lossy().into_owned();
        assert_eq!(shells.iter().map(|shell| &shell.program).collect::<Vec<_>>(), [&zsh]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
