//! The protocols our own clients do not speak (SMB, WebDAV, NFS, MTP, Google
//! Drive…), through the desktop's own bridge: KDE's kio-fuse on KDE, GNOME's
//! gvfs elsewhere when it is there. Either mounts the address as a folder,
//! which the panels browse as they do this computer's. Neither asks for a
//! password on its own: the login is passed in. On Windows and macOS the
//! system's own SMB and NFS clients do it (`system`).
#![cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// `smb://…`, `mtp:/`, `gdrive:/…`: an address, not a path. Windows' `C:/` is a path.
pub fn is_url(text: &str) -> bool {
    let Some((scheme, rest)) = text.trim().split_once(':') else {
        return false;
    };
    let named = scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    named && rest.starts_with('/') && !scheme.eq_ignore_ascii_case("file")
}

/// A share's user and password.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Login {
    pub user: String,
    pub password: String,
}

/// Mounts `url`, as `login` if given, else as a guest; the folder it is at.
/// Blocks while the server answers: run it off the UI thread.
pub fn mount(url: &str, login: Option<&Login>) -> io::Result<PathBuf> {
    #[cfg(not(any(windows, target_os = "macos")))]
    return desktop(url.trim(), login);
    #[cfg(any(windows, target_os = "macos"))]
    return system::mount(url.trim(), login);
}

/// KDE's or GNOME's.
fn desktop(url: &str, login: Option<&Login>) -> io::Result<PathBuf> {
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    let kde = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| desktop.contains("KDE"));
    let gvfs = mounts.contains(" fuse.gvfsd-fuse ");
    let mounted = match kio_fuse::available() && (kde || !gvfs) {
        true => kio_fuse::mount(url, login),
        false => gvfs::mount(url, login),
    };
    // kio-fuse may have started for it: its folder is read again.
    *ROOTS.lock().unwrap() = None;
    mounted
}

/// Where the desktops keep the folders they mount, found once.
static ROOTS: Mutex<Option<Vec<Root>>> = Mutex::new(None);

#[derive(Clone)]
enum Root {
    /// `<root>/<scheme>/<host>/<path>`.
    KioFuse(PathBuf),
    /// `<root>/smb-share:server=nas,share=media/<path>`.
    Gvfs(PathBuf),
}

/// The address a mounted folder came from, for the path bar; none for
/// this computer's own folders.
pub fn url_of(path: &Path) -> Option<String> {
    ROOTS.lock().unwrap().get_or_insert_with(read_roots).iter().find_map(|root| address_in(root, path))
}

fn read_roots() -> Vec<Root> {
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    find_roots(&mounts, runtime.as_deref())
}

/// The folder of the share `path` is in, as this app mounts shares:
/// kio-fuse's or gvfs's on Linux, `\\server\share` on Windows, a NetFS
/// mount under /Volumes on macOS; none for this computer's own folders.
/// Asks no server, so it may run as the window draws.
pub fn share_root(path: &Path) -> Option<PathBuf> {
    #[cfg(not(any(windows, target_os = "macos")))]
    return ROOTS.lock().unwrap().get_or_insert_with(read_roots).iter().find_map(|root| share_in(root, path));
    #[cfg(any(windows, target_os = "macos"))]
    return system::share_root(path);
}

/// Leaves the share at `root` (`share_root`'s): gvfs, Windows and macOS
/// let it go; kio-fuse serves every share from one mount of its own and
/// drops one nothing uses, so there is nothing to do.
pub fn leave(root: &Path) -> io::Result<()> {
    #[cfg(any(windows, target_os = "macos"))]
    return system::leave(root);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let gvfs = ROOTS.lock().unwrap().get_or_insert_with(read_roots).iter().any(|known| matches!(known, Root::Gvfs(gvfs) if root.starts_with(gvfs)));
        if !gvfs {
            return Ok(());
        }
        let done = std::process::Command::new("gio").args(["mount", "-u"]).arg(root).output()?;
        match done.status.success() {
            true => Ok(()),
            false => Err(io::Error::other(String::from_utf8_lossy(&done.stderr).trim().to_owned())),
        }
    }
}

fn find_roots(mounts: &str, runtime: Option<&Path>) -> Vec<Root> {
    let kio = mounts.lines().filter_map(|line| {
        let mut fields = line.split(' ').skip(1);
        let (path, kind) = (fields.next()?, fields.next()?);
        (kind == "fuse.kio-fuse").then(|| Root::KioFuse(PathBuf::from(path)))
    });
    let gvfs = runtime.map(|runtime| Root::Gvfs(runtime.join("gvfs")));
    kio.chain(gvfs).collect()
}

/// kio-fuse's `<root>/<scheme>/<host>`, gvfs's `<root>/<share's name>`.
fn share_in(root: &Root, path: &Path) -> Option<PathBuf> {
    match root {
        Root::KioFuse(root) => {
            let mut parts = path.strip_prefix(root).ok()?.iter();
            Some(root.join(parts.next()?).join(parts.next()?))
        }
        Root::Gvfs(root) => Some(root.join(path.strip_prefix(root).ok()?.iter().next()?)),
    }
}

fn address_in(root: &Root, path: &Path) -> Option<String> {
    let url = |scheme: &str, host: &str, rest: &Path| {
        let rest = rest.to_string_lossy();
        let rest = if rest.is_empty() { String::new() } else { format!("/{rest}") };
        format!("{scheme}://{host}{rest}")
    };
    match root {
        Root::KioFuse(root) => {
            let mut parts = path.strip_prefix(root).ok()?.iter();
            let (scheme, host) = (parts.next()?.to_str()?, parts.next()?.to_str()?);
            Some(url(scheme, host, parts.as_path()))
        }
        Root::Gvfs(root) => {
            let mut parts = path.strip_prefix(root).ok()?.iter();
            let name = parts.next()?.to_str()?;
            let (scheme, fields) = name.split_once(':')?;
            let field = |key: &str| fields.split(',').find_map(|pair| pair.strip_prefix(key)?.strip_prefix('='));
            let (scheme, host) = match (scheme, field("server"), field("share"), field("host")) {
                ("smb-share", Some(server), Some(share), _) => ("smb", format!("{server}/{share}")),
                ("dav", _, _, Some(host)) if field("ssl") == Some("true") => ("webdavs", host.to_owned()),
                ("dav", _, _, Some(host)) => ("webdav", host.to_owned()),
                (scheme, _, _, Some(host)) => (scheme, host.to_owned()),
                (scheme, Some(server), _, _) => (scheme, server.to_owned()),
                _ => return None,
            };
            let host = match field("user") {
                Some(user) => format!("{user}@{host}"),
                None => host,
            };
            Some(url(scheme, &host, parts.as_path()))
        }
    }
}

/// `url` with `user`, and `password` if given, in place of the user it has.
fn with_user(url: &str, user: &str, password: Option<&str>) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let encode = |text: &str| {
        let keep = |byte: &u8| byte.is_ascii_alphanumeric() || b"-._~".contains(byte);
        text.bytes().map(|byte| if keep(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") }).collect::<String>()
    };
    let password = password.map(|password| format!(":{}", encode(password))).unwrap_or_default();
    format!("{scheme}://{}{password}@{host}{path}", encode(user))
}

/// KDE's: `org.kde.KIOFuse` on the session bus, started on demand.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod kio_fuse {
    use std::io;
    use std::path::PathBuf;

    use zbus::blocking::Connection;

    const SERVICE: &str = "org.kde.KIOFuse";

    /// Running, or startable by D-Bus.
    pub fn available() -> bool {
        let found = || -> zbus::Result<bool> {
            let bus = Connection::session()?;
            let ask = |method: &str| -> zbus::Result<Vec<String>> {
                let names = bus.call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), method, &())?;
                names.body().deserialize()
            };
            let has = |names: Vec<String>| names.iter().any(|name| name == SERVICE);
            Ok(has(ask("ListNames")?) || has(ask("ListActivatableNames")?))
        };
        found().unwrap_or(false)
    }

    /// The password goes in the address: kio-fuse asks for none. It keeps it
    /// out of the folder's name and its messages.
    pub fn mount(url: &str, login: Option<&super::Login>) -> io::Result<PathBuf> {
        let url = match login {
            Some(login) => super::with_user(url, &login.user, Some(&login.password)),
            None => url.to_owned(),
        };
        let bus = Connection::session().map_err(io::Error::other)?;
        let reply = bus.call_method(Some(SERVICE), "/org/kde/KIOFuse", Some("org.kde.KIOFuse.VFS"), "mountUrl", &url);
        let path: String = reply.and_then(|reply| reply.body().deserialize()).map_err(|err| match err {
            // "KIOFuse failed to mount smb://nas/media: Operation not permitted": the reason.
            zbus::Error::MethodError(_, Some(text), _) => io::Error::other(text.rsplit(": ").next().unwrap_or(&text).to_owned()),
            err => io::Error::other(err),
        })?;
        Ok(PathBuf::from(path))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod kio_fuse {
    pub fn available() -> bool {
        false
    }

    pub fn mount(_: &str, _: Option<&super::Login>) -> std::io::Result<std::path::PathBuf> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

/// GNOME's: `gio mount`, then the folder gvfs-fuse gives the address.
mod gvfs {
    use std::io::{self, Read, Write};
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};

    use super::Login;

    pub fn mount(url: &str, login: Option<&Login>) -> io::Result<PathBuf> {
        // gvfs names WebDAV dav.
        let url = match url.split_once("://") {
            Some(("webdav", rest)) => format!("dav://{rest}"),
            Some(("webdavs", rest)) => format!("davs://{rest}"),
            _ => url.to_owned(),
        };
        let url = match login {
            Some(login) => super::with_user(&url, &login.user, None),
            None => url,
        };
        let mut gio = Command::new("gio");
        // Its prompts are read, in English.
        gio.arg("mount").env("LC_ALL", "C");
        if login.is_none() {
            // A guest: a share open to all needs no login.
            gio.arg("--anonymous");
        }
        let gio = gio.arg(&url).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let mut gio = gio.map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => io::Error::other(crate::t!("remote.mount.needs_desktop")),
            _ => err,
        })?;
        let refused = answer(&mut gio, login)?;
        let mounted = gio.wait_with_output()?;
        let said = String::from_utf8_lossy(&mounted.stderr).into_owned();
        // Mounted before is no failure.
        if !mounted.status.success() && !said.contains("already mounted") {
            let said = said.trim().rsplit(": ").next().unwrap_or_default();
            let why = match (refused, login) {
                (false, _) => said.to_owned(),
                (true, None) => crate::t!("remote.mount.needs_login"),
                (true, Some(_)) => crate::t!("remote.mount.login_wrong"),
            };
            return Err(io::Error::other(why));
        }
        let info = Command::new("gio").args(["info", &url]).stdin(Stdio::null()).output()?;
        let info = String::from_utf8_lossy(&info.stdout);
        let local = info.lines().find_map(|line| line.trim().strip_prefix("local path:"));
        local
            .map(|path| PathBuf::from(path.trim()))
            .ok_or_else(|| io::Error::other(crate::t!("remote.mount.no_fuse")))
    }

    /// Answers gio's prompts (`User [me]: `, `Domain [WORKGROUP]: `,
    /// `Password: `) from `login`, once: asked again, the login was wrong.
    /// True if it asked for what it was not given.
    pub(super) fn answer(gio: &mut Child, login: Option<&Login>) -> io::Result<bool> {
        let (mut input, mut output) = (gio.stdin.take(), gio.stdout.take().expect("piped"));
        let (mut said, mut byte, mut passwords) = (Vec::new(), [0u8], 0);
        while output.read(&mut byte)? == 1 {
            said.push(byte[0]);
            if !said.ends_with(b": ") {
                continue;
            }
            // The prompts share a line: each is read from the answer before.
            let text = String::from_utf8_lossy(&std::mem::take(&mut said)).into_owned();
            let prompt = text.rsplit('\n').next().unwrap_or_default();
            let reply = login.and_then(|login| match prompt {
                _ if prompt.starts_with("User") => Some(login.user.as_str()),
                // shortcut: the server's own domain; a DOMAIN\user login is not split, add it when asked for.
                _ if prompt.starts_with("Domain") => Some(""),
                _ if prompt.starts_with("Password") => {
                    passwords += 1;
                    (passwords == 1).then_some(login.password.as_str())
                }
                _ => None,
            });
            match (reply, input.as_mut()) {
                (Some(reply), Some(sink)) => writeln!(sink, "{reply}")?,
                // Its input closed, gio gives up.
                _ => input = None,
            }
        }
        Ok(input.is_none())
    }
}

/// `\\server\share\folder` for `smb://server/share/folder` (NFS's alike), and
/// the share, `\\server\share`, which a login is for.
#[cfg_attr(not(windows), allow(dead_code))]
fn unc(url: &str) -> Option<(String, String)> {
    let address = crate::remote::Address::parse(url)?;
    let path = address.path.unwrap_or_default();
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let share = format!(r"\\{}\{}", address.host, parts.first()?);
    Some((format!(r"\\{}\{}", address.host, parts.join(r"\")), share))
}

/// Windows' own clients: the address as a UNC path; a login given is
/// Windows' for the share until the user logs off. NFS needs Windows' Client for NFS.
#[cfg(windows)]
mod system {
    use std::io;
    use std::path::PathBuf;

    use std::path::Path;

    use windows::Win32::NetworkManagement::WNet::{CONNECT_TEMPORARY, NET_CONNECT_FLAGS, NETRESOURCEW, RESOURCETYPE_DISK, WNetAddConnection2W, WNetCancelConnection2W};
    use windows::core::{HSTRING, PWSTR};

    use super::Login;

    /// `\\server\share` of a path in it.
    pub fn share_root(path: &Path) -> Option<PathBuf> {
        use std::path::{Component, Prefix};
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return None;
        };
        match prefix.kind() {
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => Some(PathBuf::from(format!(r"\\{}\{}", server.display(), share.display()))),
            _ => None,
        }
    }

    /// The connection to the share, Windows' until logging off, ended; none there is fine.
    pub fn leave(root: &Path) -> io::Result<()> {
        const NOT_CONNECTED: u32 = 2250;
        match unsafe { WNetCancelConnection2W(&HSTRING::from(root.as_os_str()), NET_CONNECT_FLAGS(0), false) }.0 {
            0 | NOT_CONNECTED => Ok(()),
            code => Err(io::Error::from_raw_os_error(code as i32)),
        }
    }

    pub fn mount(url: &str, login: Option<&Login>) -> io::Result<PathBuf> {
        let (path, share) = super::unc(url).ok_or_else(|| io::Error::other(crate::t!("remote.mount.no_share")))?;
        if let Some(login) = login {
            let mut remote: Vec<u16> = share.encode_utf16().chain([0]).collect();
            let resource = NETRESOURCEW { dwType: RESOURCETYPE_DISK, lpRemoteName: PWSTR(remote.as_mut_ptr()), ..Default::default() };
            let (password, user) = (HSTRING::from(login.password.as_str()), HSTRING::from(login.user.as_str()));
            let added = unsafe { WNetAddConnection2W(&resource, &password, &user, CONNECT_TEMPORARY) };
            match added.0 {
                0 => {}
                1326 => return Err(io::Error::other(crate::t!("remote.mount.login_wrong"))),
                1219 => return Err(io::Error::other(crate::t!("remote.mount.other_user"))),
                code => return Err(io::Error::from_raw_os_error(code as i32)),
            }
        }
        // Open to guests, or to the login Windows has for it: reading it tells.
        let nfs = url.get(..6).is_some_and(|scheme| scheme.eq_ignore_ascii_case("nfs://"));
        std::fs::read_dir(&path).map(|_| PathBuf::from(&path)).map_err(|err| match err.raw_os_error() {
            Some(67) if nfs => io::Error::other(crate::t!("remote.mount.no_nfs_client")),
            _ => err,
        })
    }
}

/// macOS's own, as Finder mounts: NetFS, under /Volumes, showing no dialog of its own.
#[cfg(target_os = "macos")]
mod system {
    use std::ffi::{CStr, c_int};
    use std::io;
    use std::path::PathBuf;
    use std::ptr::NonNull;

    use objc2_core_foundation::{CFArray, CFBoolean, CFMutableDictionary, CFRetained, CFString, CFType, CFURL};

    use super::Login;

    #[link(name = "NetFS", kind = "framework")]
    unsafe extern "C" {
        fn NetFSMountURLSync(
            url: &CFURL,
            mountpath: Option<&CFURL>,
            user: Option<&CFString>,
            passwd: Option<&CFString>,
            open_options: Option<&CFMutableDictionary>,
            mount_options: Option<&CFMutableDictionary>,
            mountpoints: *mut Option<NonNull<CFArray>>,
        ) -> c_int;
    }

    /// Mounted already.
    const EEXIST: c_int = 17;
    /// The login was refused.
    const EAUTH: c_int = 80;

    pub fn mount(url: &str, login: Option<&Login>) -> io::Result<PathBuf> {
        let address = crate::remote::Address::parse(url).ok_or_else(|| io::Error::other(crate::t!("remote.mount.not_address")))?;
        let path = address.path.clone().unwrap_or_default();
        let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        // SMB mounts the share, the rest is a folder in it; NFS the path as given.
        let smb = address.scheme == crate::remote::Scheme::Smb;
        let (mounted, rest) = match smb {
            true => (&parts[..parts.len().min(1)], &parts[parts.len().min(1)..]),
            false => (&parts[..], &parts[..0]),
        };
        if mounted.is_empty() {
            return Err(io::Error::other(crate::t!("remote.mount.no_share")));
        }
        let host = if address.host.contains(':') { format!("[{}]", address.host) } else { address.host.clone() };
        let port = address.port.map(|port| format!(":{port}")).unwrap_or_default();
        let target = format!("{}://{host}{port}/{}", address.scheme.name(), encode(&mounted.join("/")));
        let url = CFURL::from_string(None, &CFString::from_str(&target), None).ok_or_else(|| io::Error::other(crate::t!("remote.mount.not_address")))?;
        let options = CFMutableDictionary::<CFString, CFType>::empty();
        options.set(&CFString::from_str("UIOption"), &CFString::from_str("NoUI"));
        if login.is_none() {
            options.set(&CFString::from_str("Guest"), CFBoolean::new(true));
        }
        let (user, password) = (login.map(|login| CFString::from_str(&login.user)), login.map(|login| CFString::from_str(&login.password)));
        let mut points = None;
        let code = unsafe { NetFSMountURLSync(&url, None, user.as_deref(), password.as_deref(), Some(options.as_ref()), None, &mut points) };
        let points = points.map(|points| unsafe { CFRetained::<CFArray<CFString>>::from_raw(points.cast()) });
        let at = match code {
            0 => points.and_then(|points| points.get(0)).map(|point| PathBuf::from(point.to_string())),
            EEXIST => mounted_at(&address.host, &mounted.join("/")),
            EAUTH if login.is_some() => return Err(io::Error::other(crate::t!("remote.mount.login_wrong"))),
            EAUTH => return Err(io::Error::other(crate::t!("remote.mount.needs_login"))),
            code if code > 0 => return Err(io::Error::from_raw_os_error(code)),
            code => return Err(io::Error::other(crate::t!("remote.mount.macos_failed", code = code))),
        };
        let at = at.ok_or_else(|| io::Error::other(crate::t!("remote.mount.not_found")))?;
        Ok(rest.iter().fold(at, |at, part| at.join(part)))
    }

    /// Spaces and such escaped, as a URL's path has them.
    fn encode(path: &str) -> String {
        let keep = |byte: &u8| byte.is_ascii_alphanumeric() || b"-._~/".contains(byte);
        path.bytes().map(|byte| if keep(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") }).collect()
    }

    /// The mounts the system has, without waiting for any server: what each
    /// is of (lowercase), where it is, and its kind.
    fn mounts() -> Vec<(String, PathBuf, String)> {
        let mut mounts: *mut libc::statfs = std::ptr::null_mut();
        let count = unsafe { libc::getmntinfo(&mut mounts, libc::MNT_NOWAIT) };
        if count <= 0 || mounts.is_null() {
            return Vec::new();
        }
        let text = |field: &[libc::c_char]| unsafe { CStr::from_ptr(field.as_ptr()) }.to_string_lossy().into_owned();
        let mounts = unsafe { std::slice::from_raw_parts(mounts, count as usize) };
        mounts.iter().map(|mount| (text(&mount.f_mntfromname).to_lowercase(), PathBuf::from(text(&mount.f_mntonname)), text(&mount.f_fstypename))).collect()
    }

    /// Where `host`'s `share` (an NFS export: its path) is mounted already.
    fn mounted_at(host: &str, share: &str) -> Option<PathBuf> {
        let (host, share) = (host.to_lowercase(), share.trim_matches('/').to_lowercase());
        mounts().into_iter().find_map(|(from, on, _)| {
            // smbfs: //user@host/share; nfs: host:/export.
            let smb = from.trim_start_matches('/').rsplit('@').next() == Some(&format!("{host}/{share}"));
            let nfs = from == format!("{host}:/{share}");
            (smb || nfs).then_some(on)
        })
    }

    /// The network share a path is in.
    pub fn share_root(path: &std::path::Path) -> Option<PathBuf> {
        const NETWORK: [&str; 4] = ["smbfs", "nfs", "webdav", "afpfs"];
        let shares = mounts().into_iter().filter(|(_, on, kind)| NETWORK.contains(&kind.as_str()) && path.starts_with(on));
        shares.map(|(_, on, _)| on).max_by_key(|on| on.as_os_str().len())
    }

    pub fn leave(root: &std::path::Path) -> io::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let root = std::ffi::CString::new(root.as_os_str().as_bytes())?;
        match unsafe { libc::unmount(root.as_ptr(), 0) } {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_share_a_folder_is_in() {
        let kio = Root::KioFuse("/run/user/1000/kio-fuse-ab12".into());
        let share = |root: &Root, path: &str| share_in(root, Path::new(path));
        assert_eq!(share(&kio, "/run/user/1000/kio-fuse-ab12/webdavs/me@dav.example/docs/a").unwrap(), Path::new("/run/user/1000/kio-fuse-ab12/webdavs/me@dav.example"));
        assert_eq!(share(&kio, "/run/user/1000/kio-fuse-ab12/webdavs"), None, "no server yet");
        assert_eq!(share(&kio, "/home/me"), None);
        let gvfs = Root::Gvfs("/run/user/1000/gvfs".into());
        let named = "/run/user/1000/gvfs/smb-share:server=nas,share=media";
        assert_eq!(share(&gvfs, &format!("{named}/Films")).unwrap(), Path::new(named));
    }

    #[test]
    fn makes_windows_share_paths() {
        assert_eq!(unc("smb://me@nas/media/Films/2024").unwrap(), (r"\\nas\media\Films\2024".into(), r"\\nas\media".into()));
        assert_eq!(unc("nfs://srv/export").unwrap().0, r"\\srv\export");
        assert_eq!(unc("smb://nas"), None, "a share is needed");
    }

    #[test]
    fn tells_addresses_from_paths() {
        for url in ["smb://nas/share", "mtp:/", "gdrive:/My Drive", "webdavs://cloud.example.org/remote.php"] {
            assert!(is_url(url), "{url}");
        }
        for path in ["/home/me", "C:/Windows", "file:///home", "notes.txt", "a:b"] {
            assert!(!is_url(path), "{path}");
        }
    }

    #[test]
    fn puts_logins_in_addresses() {
        let login = with_user("smb://old@nas:445/media/Films", "me", Some("p@ss:wörd"));
        assert_eq!(login, "smb://me:p%40ss%3Aw%C3%B6rd@nas:445/media/Films");
        assert_eq!(with_user("webdav://cloud.example.org", "me", None), "webdav://me@cloud.example.org");
    }

    #[cfg(unix)]
    #[test]
    fn answers_gios_prompts() {
        use std::process::{Command, Stdio};
        let gio = |script: &str, login: Option<&Login>| {
            let (piped, out, err) = (Stdio::piped(), Stdio::piped(), Stdio::piped());
            let mut shell = Command::new("sh").args(["-c", script]).stdin(piped).stdout(out).stderr(err).spawn().unwrap();
            let refused = gvfs::answer(&mut shell, login).unwrap();
            (refused, String::from_utf8(shell.wait_with_output().unwrap().stderr).unwrap())
        };
        let login = Login { user: "me".into(), password: "secret".into() };
        let asks = "printf 'Enter user and password:\\nUser [x]: '; read u; printf 'Domain [W]: '; read d; printf 'Password: '; read p; echo \"$u|$d|$p\" >&2";
        assert_eq!(gio(asks, Some(&login)), (false, "me||secret\n".into()));
        // A guest is asked nothing it cannot answer.
        assert!(gio(asks, None).0);
        // Asked again, the password was wrong: no second try.
        let again = "printf 'Password: '; read p; printf 'Password: '; read q || echo gave up >&2";
        assert_eq!(gio(again, Some(&login)), (true, "gave up\n".into()));
    }

    #[test]
    fn maps_mounted_folders_back_to_addresses() {
        let mounts = "/dev/sda1 / btrfs rw 0 0\nkio-fuse /run/user/1000/kio-fuse-AbC fuse.kio-fuse rw 0 0\n";
        let roots = find_roots(mounts, Some(Path::new("/run/user/1000")));
        let url = |path: &str| roots.iter().find_map(|root| address_in(root, Path::new(path)));
        assert_eq!(url("/run/user/1000/kio-fuse-AbC/smb/nas/media/Films").as_deref(), Some("smb://nas/media/Films"));
        assert_eq!(url("/run/user/1000/kio-fuse-AbC/webdav/127.0.0.1:8088").as_deref(), Some("webdav://127.0.0.1:8088"));
        let gvfs = "/run/user/1000/gvfs/smb-share:server=nas,share=media/Films";
        assert_eq!(url(gvfs).as_deref(), Some("smb://nas/media/Films"));
        let gvfs = "/run/user/1000/gvfs/smb-share:server=nas,share=media,user=me";
        assert_eq!(url(gvfs).as_deref(), Some("smb://me@nas/media"));
        let dav = "/run/user/1000/gvfs/dav:host=cloud.example.org,ssl=true/docs";
        assert_eq!(url(dav).as_deref(), Some("webdavs://cloud.example.org/docs"));
        assert_eq!(url("/home/me"), None);
    }
}
