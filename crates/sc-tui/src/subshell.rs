//! Ctrl+O, as Midnight Commander's: the user's shell in a terminal of its own
//! behind the panels, living on between switches. Shown, it has the screen and
//! the keys; Ctrl+O at its prompt brings the panels back (a program running in
//! it gets Ctrl+O, as nano's Write Out).

use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const CTRL_O: u8 = 0x0f;
// shortcut: what it writes behind the panels is kept up to 1 MB, the end; a cut escape sequence may garble a line.
const HELD: usize = 1 << 20;

pub struct Subshell {
    pty: File,
    child: Child,
    /// Whether its output goes to the screen; else what it wrote meanwhile.
    shown: Arc<Mutex<(bool, Vec<u8>)>>,
    ended: Arc<AtomicBool>,
}

impl Subshell {
    /// `program` (the settings' shell), else the user's own, in `dir`.
    pub fn start(dir: &Path, program: Option<(String, Vec<String>)>) -> io::Result<Subshell> {
        let (pty, terminal, name) = open_pty()?;
        let (program, args) = program.unwrap_or_else(|| (sc_core::system::shells::default_name(), Vec::new()));
        let mut command = Command::new(program);
        command.args(args).current_dir(dir).env(crate::IN_TERMINAL, name);
        command.stdin(Stdio::from(terminal.try_clone()?)).stdout(Stdio::from(terminal.try_clone()?)).stderr(Stdio::from(terminal));
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            command.pre_exec(|| {
                // A session of its own, the pty its terminal: job control and Ctrl+C work in it.
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        let shown = Arc::new(Mutex::new((false, Vec::new())));
        let ended = Arc::new(AtomicBool::new(false));
        let (mut reader, heard, gone) = (pty.try_clone()?, shown.clone(), ended.clone());
        std::thread::spawn(move || {
            let mut chunk = [0; 8192];
            loop {
                let read = match reader.read(&mut chunk) {
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                let mut state = heard.lock().unwrap_or_else(|err| err.into_inner());
                match state.0 {
                    true => {
                        let mut out = io::stdout().lock();
                        let _ = out.write_all(&chunk[..read]).and_then(|()| out.flush());
                    }
                    false => {
                        state.1.extend_from_slice(&chunk[..read]);
                        let over = state.1.len().saturating_sub(HELD);
                        state.1.drain(..over);
                    }
                }
            }
            gone.store(true, Ordering::Relaxed);
        });
        Ok(Subshell { pty, child, shown, ended })
    }

    pub fn ended(&mut self) -> bool {
        self.ended.load(Ordering::Relaxed) || self.child.try_wait().ok().flatten().is_some()
    }

    /// The folder it is in.
    pub fn folder(&self) -> Option<PathBuf> {
        folder_of(self.child.id() as i32)
    }

    /// Typed at its prompt, as mc does; the space keeps it out of the history.
    pub fn cd(&mut self, dir: &Path) {
        let quoted = String::from_utf8_lossy(dir.as_os_str().as_bytes()).replace('\'', r"'\''");
        let _ = writeln!(self.pty, " cd '{quoted}'");
    }

    /// Whether it waits at its prompt, nothing running in front of it.
    fn idle(&self) -> bool {
        // SAFETY: asks the pty, which `self` keeps open, who is in front.
        unsafe { libc::tcgetpgrp(self.pty.as_raw_fd()) == self.child.id() as i32 }
    }

    /// The screen and the keys are its own (the terminal raw, the panels'
    /// screen left) until Ctrl+O at its prompt or its end; true when it ended.
    pub fn show(&mut self) -> io::Result<bool> {
        {
            let mut state = self.shown.lock().unwrap_or_else(|err| err.into_inner());
            let mut out = io::stdout().lock();
            out.write_all(&std::mem::take(&mut state.1))?;
            out.flush()?;
            state.0 = true;
        }
        let relayed = self.relay();
        self.shown.lock().unwrap_or_else(|err| err.into_inner()).0 = false;
        relayed
    }

    fn relay(&mut self) -> io::Result<bool> {
        let (mut size, mut keys) = (None, [0; 1024]);
        loop {
            if self.ended() {
                return Ok(true);
            }
            let now = ratatui::crossterm::terminal::size().ok();
            if now != size {
                if let Some((columns, rows)) = now {
                    let size = libc::winsize { ws_row: rows, ws_col: columns, ws_xpixel: 0, ws_ypixel: 0 };
                    // SAFETY: sets the pty's size from a whole winsize.
                    unsafe { libc::ioctl(self.pty.as_raw_fd(), libc::TIOCSWINSZ, &size) };
                }
                size = now;
            }
            let mut stdin = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
            // SAFETY: one pollfd, its count one; a short wait so an end or a new size is seen.
            if unsafe { libc::poll(&mut stdin, 1, 50) } <= 0 {
                continue;
            }
            // SAFETY: reads into a buffer of the length given.
            let read = unsafe { libc::read(0, keys.as_mut_ptr().cast(), keys.len()) };
            if read <= 0 {
                continue;
            }
            let keys = &keys[..read as usize];
            match keys.iter().position(|&key| key == CTRL_O) {
                Some(at) if self.idle() => {
                    self.pty.write_all(&keys[..at])?;
                    return Ok(false);
                }
                _ => self.pty.write_all(keys)?,
            }
        }
    }
}

impl Drop for Subshell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A pseudo-terminal: our end, the shell's, and its name.
fn open_pty() -> io::Result<(File, OwnedFd, String)> {
    let failed = |code: i32| if code < 0 { Err(io::Error::last_os_error()) } else { Ok(code) };
    // SAFETY: the C calls get valid descriptors and a name copied before the next call.
    unsafe {
        let ours = OwnedFd::from_raw_fd(failed(libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY))?);
        failed(libc::fcntl(ours.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC))?;
        failed(libc::grantpt(ours.as_raw_fd()))?;
        failed(libc::unlockpt(ours.as_raw_fd()))?;
        let name = libc::ptsname(ours.as_raw_fd());
        if name.is_null() {
            return Err(io::Error::last_os_error());
        }
        let name = CString::from(std::ffi::CStr::from_ptr(name));
        let theirs = OwnedFd::from_raw_fd(failed(libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC))?);
        Ok((File::from(ours), theirs, name.to_string_lossy().into_owned()))
    }
}

#[cfg(target_os = "linux")]
fn folder_of(pid: i32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(target_os = "macos")]
fn folder_of(pid: i32) -> Option<PathBuf> {
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as i32;
    // SAFETY: the kernel fills a whole proc_vnodepathinfo of the size given.
    let got = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDVNODEPATHINFO, 0, (&raw mut info).cast(), size) };
    if got != size {
        return None;
    }
    // SAFETY: a NUL-ended path the kernel wrote.
    let path = unsafe { std::ffi::CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr().cast()) };
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes())))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn folder_of(_: i32) -> Option<PathBuf> {
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn goes_where_told_and_tells_where_it_is() {
        let dir = std::env::temp_dir().join(format!("sc-subshell-{}", std::process::id()));
        let inner = dir.join("it's here");
        std::fs::create_dir_all(&inner).unwrap();
        let mut shell = Subshell::start(&dir, Some(("/bin/sh".into(), Vec::new()))).unwrap();
        let waited = |shell: &Subshell, done: &dyn Fn(&Subshell) -> bool| {
            let start = Instant::now();
            while !done(shell) && start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(20));
            }
            done(shell)
        };
        assert!(waited(&shell, &|shell| shell.idle() && shell.folder().as_deref() == Some(dir.as_path())));
        shell.cd(&inner);
        assert!(waited(&shell, &|shell| shell.folder().as_deref() == Some(inner.as_path())));
        let _ = writeln!(shell.pty, "exit");
        assert!(waited(&shell, &|shell| shell.ended.load(Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
