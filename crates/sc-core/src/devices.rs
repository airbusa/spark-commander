//! The sidebar's devices: disks, cards, sticks and network shares as the
//! system mounts them, the cloud apps' folders, and phones paired through KDE Connect. A phone's
//! files, once mounted, are ordinary folders: KDE Connect mounts them with
//! sshfs under the runtime folder, as its own `kdeconnect:/` does.

use std::io;
use std::path::{Path, PathBuf};

use crate::remote::Ask;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VolumeKind {
    /// macOS's start-up disk, Windows's C:.
    System,
    Drive,
    Removable,
    /// A cloud app's folder (OneDrive, iCloud Drive…), or a mount of one.
    Cloud,
    Network,
    /// GNOME's phones and cameras (MTP, PTP, AFC).
    Phone,
}

/// A disk, card, stick or share; mounted, or mountable by udisks2.
#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    pub name: String,
    pub kind: VolumeKind,
    /// Where it is mounted.
    pub path: Option<PathBuf>,
    /// udisks2's object, which mounts and unmounts it.
    pub udisks: Option<String>,
    /// Encrypted (LUKS): udisks2's object of the container, unlocked by a passphrase, locked once unmounted.
    pub encrypted: Option<String>,
}

/// What is mounted or mountable, by kind, then by name.
pub fn volumes() -> Vec<Volume> {
    let mut volumes = platform_volumes();
    volumes.extend(dirs::home_dir().map(|home| cloud_folders(&home)).unwrap_or_default());
    volumes.sort_by_cached_key(|volume| (volume.kind, volume.name.to_lowercase()));
    volumes.dedup_by(|a, b| a.path == b.path);
    volumes
}

/// Mounts a volume udisks2 knows, an encrypted one unlocked first with the
/// passphrase `ask` gets; where it went. Blocks on the user: a thread of its own.
pub fn mount_volume(volume: &Volume, ask: &Ask) -> io::Result<PathBuf> {
    match (&volume.path, &volume.udisks, &volume.encrypted) {
        (Some(path), ..) => Ok(path.clone()),
        (None, Some(object), _) => udisks::mount(object),
        (None, None, Some(container)) => udisks::unlock(container, &volume.name, ask),
        (None, None, None) => Err(io::Error::from(io::ErrorKind::Unsupported)),
    }
}

/// An encrypted one is locked again after, as Dolphin does, so the disk may go.
pub fn unmount_volume(volume: &Volume) -> io::Result<()> {
    match &volume.udisks {
        Some(object) => {
            udisks::unmount(object)?;
            volume.encrypted.as_deref().map_or(Ok(()), udisks::lock)
        }
        None => Err(io::Error::other("only the system can unmount this")),
    }
}

/// A mounted volume's size and the space its user may still fill, in bytes.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn disk_space(path: &Path) -> io::Result<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: a zeroed statvfs is valid, and the call fills it.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let unit = stat.f_frsize as u64;
    Ok((stat.f_blocks as u64 * unit, stat.f_bavail as u64 * unit))
}

#[cfg(windows)]
pub fn disk_space(path: &Path) -> io::Result<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: a NUL-ended path, and two u64s for the answers.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), Some(&mut total), None) }.map_err(io::Error::other)?;
    Ok((total, free))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn disk_space(_: &Path) -> io::Result<(u64, u64)> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(target_os = "linux")]
fn platform_volumes() -> Vec<Volume> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    // shortcut: without udisks2, drives are missing; shares and GNOME's mounts still show.
    let mut volumes = udisks::volumes().unwrap_or_default();
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    volumes.extend(network_mounts(&mounts, runtime.as_deref()));
    volumes.extend(runtime.map(|dir| gvfs_mounts(&dir.join("gvfs"))).unwrap_or_default());
    volumes
}

#[cfg(target_os = "macos")]
fn platform_volumes() -> Vec<Volume> {
    let entries = std::fs::read_dir("/Volumes").into_iter().flatten().flatten();
    let volumes = entries.map(|entry| {
        let path = entry.path();
        // The start-up disk is a link to /.
        let system = std::fs::canonicalize(&path).is_ok_and(|real| real == Path::new("/"));
        Volume {
            name: entry.file_name().to_string_lossy().into_owned(),
            kind: if system { VolumeKind::System } else { VolumeKind::Drive },
            path: Some(path),
            udisks: None,
            encrypted: None,
        }
    });
    volumes.collect()
}

#[cfg(windows)]
fn platform_volumes() -> Vec<Volume> {
    use windows::Win32::NetworkManagement::WNet::WNetGetConnectionW;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    use windows::core::{HSTRING, PWSTR};
    const REMOVABLE: u32 = 2;
    const FIXED: u32 = 3;
    const REMOTE: u32 = 4;
    const CDROM: u32 = 5;
    let text = |buffer: &[u16]| String::from_utf16_lossy(&buffer[..buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len())]);
    let label = |root: &str| {
        let mut name = [0u16; 261];
        unsafe { GetVolumeInformationW(&HSTRING::from(root), Some(&mut name), None, None, None, None) }.ok()?;
        Some(text(&name)).filter(|name| !name.is_empty())
    };
    // The share it stands for, as Explorer shows it: "vmshare (\\nas)"; asked of Windows, not of the server.
    let share = |drive: &str| {
        let (mut unc, mut length) = ([0u16; 1024], 1024u32);
        let found = unsafe { WNetGetConnectionW(&HSTRING::from(drive), Some(PWSTR(unc.as_mut_ptr())), &mut length) };
        let unc = (found.0 == 0).then(|| text(&unc))?;
        Some(match unc.rsplit_once('\\') {
            Some((server, share)) if !server.is_empty() => format!("{share} ({server})"),
            _ => unc,
        })
    };
    let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_uppercase();
    let drives = unsafe { GetLogicalDrives() };
    // From C: on: A: and B: are floppies, slow to ask.
    let letters = (b'C'..=b'Z').filter(|letter| drives & (1 << (letter - b'A')) != 0);
    let volumes = letters.filter_map(|letter| {
        let (drive, root) = (format!("{}:", letter as char), format!("{}:\\", letter as char));
        let (kind, named) = match unsafe { GetDriveTypeW(&HSTRING::from(&root)) } {
            REMOTE => (VolumeKind::Network, share(&drive)),
            REMOVABLE | CDROM => (VolumeKind::Removable, label(&root)),
            FIXED if drive == system => (VolumeKind::System, label(&root)),
            FIXED => (VolumeKind::Drive, label(&root)),
            _ => return None,
        };
        // As Explorer names them: "Windows (C:)".
        let name = named.map_or(drive.clone(), |named| format!("{named} ({drive})"));
        Some(Volume { name, kind, path: Some(root.into()), udisks: None, encrypted: None })
    });
    volumes.collect()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn platform_volumes() -> Vec<Volume> {
    Vec::new()
}

/// The cloud apps' folders in the home folder (OneDrive, Dropbox, Box, iCloud
/// for Windows), and macOS's: iCloud Drive and File Provider's `CloudStorage`.
fn cloud_folders(home: &Path) -> Vec<Volume> {
    let cloud = |name: String, path: PathBuf| Volume { name, kind: VolumeKind::Cloud, path: Some(path), udisks: None, encrypted: None };
    let in_home = std::fs::read_dir(home).into_iter().flatten().flatten().filter_map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        let known = name.starts_with("OneDrive") || name.starts_with("Dropbox") || ["Box", "iCloudDrive"].contains(&name.as_str());
        let name = if name == "iCloudDrive" { "iCloud Drive".to_owned() } else { name };
        (known && entry.path().is_dir()).then(|| cloud(name, entry.path()))
    });
    // OneDrive's own, wherever it was moved to.
    let moved = ["OneDriveConsumer", "OneDriveCommercial"].into_iter().filter(|_| cfg!(windows)).filter_map(std::env::var_os);
    let moved = moved.map(PathBuf::from).filter(|path| path.is_dir());
    let moved = moved.map(|path| cloud(path.file_name().unwrap_or_default().to_string_lossy().into_owned(), path));
    // The providers' folders, not Finder's .DS_Store beside them.
    let storage = std::fs::read_dir(home.join("Library/CloudStorage")).into_iter().flatten().flatten();
    let storage = storage.filter_map(|entry| {
        let folder = entry.file_name().to_string_lossy().into_owned();
        (!folder.starts_with('.') && entry.path().is_dir()).then(|| cloud(provider_name(&folder), entry.path()))
    });
    let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    let icloud = icloud.is_dir().then(|| cloud("iCloud Drive".to_owned(), icloud));
    in_home.chain(moved).chain(storage).chain(icloud).collect()
}

/// `GoogleDrive-me@gmail.com` is "Google Drive (me@gmail.com)", `Box-Box` "Box".
fn provider_name(folder: &str) -> String {
    let (app, account) = folder.split_once('-').unwrap_or((folder, ""));
    let app = if app == "GoogleDrive" { "Google Drive" } else { app };
    match account {
        "" => app.to_owned(),
        account if account == app => app.to_owned(),
        account => format!("{app} ({account})"),
    }
}

/// Network file systems of `/proc/self/mounts`, but KDE Connect's phones,
/// which it mounts right in the runtime folder.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn network_mounts(mounts: &str, runtime: Option<&Path>) -> Vec<Volume> {
    const NETWORK: [&str; 9] =
        ["cifs", "smb3", "smbfs", "nfs", "nfs4", "fuse.sshfs", "fuse.rclone", "davfs", "fuse.davfs"];
    let shares = mounts.lines().filter_map(|line| {
        let mut fields = line.split(' ').skip(1);
        let (path, kind) = (unescape(fields.next()?), fields.next()?);
        let phone = runtime.is_some_and(|runtime| path.parent() == Some(runtime));
        (NETWORK.contains(&kind) && !phone).then(|| Volume {
            name: path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned()),
            kind: if kind == "fuse.rclone" { VolumeKind::Cloud } else { VolumeKind::Network },
            path: Some(path),
            udisks: None,
            encrypted: None,
        })
    });
    shares.collect()
}

/// `/proc/mounts` writes a space as `\040`.
fn unescape(field: &str) -> PathBuf {
    let mut bytes = Vec::new();
    let mut rest = field.as_bytes();
    while let Some((&first, tail)) = rest.split_first() {
        let octal = tail.get(..3).and_then(|digits| u8::from_str_radix(std::str::from_utf8(digits).ok()?, 8).ok());
        match (first, octal) {
            (b'\\', Some(byte)) => {
                bytes.push(byte);
                rest = &tail[3..];
            }
            _ => {
                bytes.push(first);
                rest = tail;
            }
        }
    }
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

/// GNOME mounts shares and phones as folders of `gvfs`, named after their
/// address: `smb-share:server=nas,share=media`, `mtp:host=SAMSUNG_Android`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn gvfs_mounts(dir: &Path) -> Vec<Volume> {
    let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
    let volumes = entries.map(|entry| {
        let raw = entry.file_name().to_string_lossy().into_owned();
        let (scheme, rest) = raw.split_once(':').unwrap_or(("", &raw));
        let field = |key: &str| rest.split(',').find_map(|pair| pair.strip_prefix(key)?.strip_prefix('='));
        let app = match scheme {
            "google-drive" => Some("Google Drive"),
            "onedrive" => Some("OneDrive"),
            _ => None,
        };
        let name = match (app, field("share"), field("server").or(field("host"))) {
            (Some(app), ..) => match (field("user"), field("host")) {
                (Some(user), Some(host)) => format!("{app} ({user}@{host})"),
                _ => app.to_owned(),
            },
            (None, Some(share), Some(server)) => format!("{share} on {server}"),
            (None, None, Some(server)) => server.replace('_', " "),
            _ => raw.clone(),
        };
        Volume {
            name,
            kind: match scheme {
                "mtp" | "gphoto2" | "afc" => VolumeKind::Phone,
                _ if app.is_some() => VolumeKind::Cloud,
                _ => VolumeKind::Network,
            },
            path: Some(entry.path()),
            udisks: None,
            encrypted: None,
        }
    });
    volumes.collect()
}

/// udisks2 on the system bus: what Dolphin and Nautilus list as devices.
#[cfg(target_os = "linux")]
mod udisks {
    use std::collections::HashMap;
    use std::io;
    use std::path::{Path, PathBuf};

    use zbus::blocking::Connection;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    use super::{Volume, VolumeKind};
    use crate::remote::{Ask, Prompt, Reply};

    const SERVICE: &str = "org.freedesktop.UDisks2";
    const BLOCK: &str = "org.freedesktop.UDisks2.Block";
    const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
    const ENCRYPTED: &str = "org.freedesktop.UDisks2.Encrypted";
    const LOOP: &str = "org.freedesktop.UDisks2.Loop";
    type Properties = HashMap<String, OwnedValue>;

    fn get<T: TryFrom<OwnedValue>>(properties: Option<&Properties>, name: &str) -> Option<T> {
        T::try_from(properties?.get(name)?.try_clone().ok()?).ok()
    }

    /// The system's own places: what is mounted there is not unmounted from here.
    fn system_place(path: &Path) -> bool {
        let places = ["/boot", "/home", "/var", "/usr", "/opt", "/srv", "/swap", "/nix", "/gnu"];
        path == Path::new("/") || places.iter().any(|place| path.starts_with(place))
    }

    pub fn volumes() -> zbus::Result<Vec<Volume>> {
        let bus = Connection::system()?;
        let manager = "org.freedesktop.DBus.ObjectManager";
        let reply = bus.call_method(Some(SERVICE), "/org/freedesktop/UDisks2", Some(manager), "GetManagedObjects", &())?;
        let objects: HashMap<OwnedObjectPath, HashMap<String, Properties>> = reply.body().deserialize()?;
        let set = |path: OwnedObjectPath| (path.as_str() != "/").then_some(path);
        let volumes = objects.iter().filter_map(|(object, interfaces)| {
            let block = interfaces.get(BLOCK);
            if get(block, "HintIgnore").unwrap_or(false) {
                return None;
            }
            // Locked, the container stands for it; unlocked, the file system in it.
            let locked = interfaces.get(ENCRYPTED).is_some_and(|encrypted| get::<OwnedObjectPath>(Some(encrypted), "CleartextDevice").and_then(set).is_none());
            let filesystem = interfaces.get(FILESYSTEM);
            if filesystem.is_none() && !locked {
                return None;
            }
            let container = get::<OwnedObjectPath>(block, "CryptoBackingDevice").and_then(set);
            let encrypted = match locked {
                true => Some(object.to_string()),
                false => container.as_ref().map(|container| container.to_string()),
            };
            let outer = container.as_ref().and_then(|container| objects.get(container)?.get(BLOCK));
            let points: Vec<Vec<u8>> = get(filesystem, "MountPoints").unwrap_or_default();
            let points: Vec<PathBuf> = points
                .into_iter()
                .map(|point| PathBuf::from(String::from_utf8_lossy(point.strip_suffix(&[0]).unwrap_or(&point)).into_owned()))
                .collect();
            // An unlocked one's disk is its container's.
            let drive = get::<OwnedObjectPath>(outer.or(block), "Drive").and_then(set);
            let drive = drive.and_then(|drive| objects.get(&drive)?.get("org.freedesktop.UDisks2.Drive"));
            // A loop device's image is the system's (snaps), unless the user set it up (an ISO) or it is in the media folders.
            let in_media = points.iter().any(|point| point.starts_with("/media") || point.starts_with("/run/media"));
            let image = objects.get(container.as_ref().unwrap_or(object)).and_then(|interfaces| interfaces.get(LOOP));
            // SAFETY: getuid cannot fail.
            let own = get::<u32>(image, "SetupByUID") == Some(unsafe { libc::getuid() });
            if drive.is_none() && !in_media && !own {
                return None;
            }
            let bus_name: String = get(drive, "ConnectionBus").unwrap_or_default();
            let removable = get(drive, "Removable").unwrap_or(false)
                || get(drive, "MediaRemovable").unwrap_or(false)
                || ["usb", "sdio", "ieee1394"].contains(&bus_name.as_str());
            let label = get::<String>(block, "HintName").filter(|name| !name.is_empty());
            let label = label.or_else(|| get::<String>(block, "IdLabel").filter(|name| !name.is_empty()));
            let label = label.or_else(|| get::<String>(outer, "IdLabel").filter(|name| !name.is_empty()));
            let size = get::<u64>(block, "Size").unwrap_or(0);
            let unnamed = if encrypted.is_some() { "encrypted volume" } else { "volume" };
            let path = main_point(&points, label.as_deref());
            let kind = match path.as_deref() {
                Some(path) if path == Path::new("/") => VolumeKind::System,
                _ if removable => VolumeKind::Removable,
                _ => VolumeKind::Drive,
            };
            let system = points.iter().any(|point| system_place(point));
            Some(Volume {
                name: label.unwrap_or_else(|| format!("{:.1} GB {unnamed}", size as f64 / 1e9)),
                kind,
                path,
                udisks: (!system && !locked).then(|| object.to_string()),
                encrypted,
            })
        });
        Ok(volumes.collect())
    }

    /// Of a file system's mounts (btrfs subvolumes): the system's root, else
    /// the one named as the volume, else the first, as Dolphin opens.
    pub(super) fn main_point(points: &[PathBuf], label: Option<&str>) -> Option<PathBuf> {
        let named = |point: &&PathBuf| label.is_some_and(|label| point.file_name().is_some_and(|name| name.eq_ignore_ascii_case(label)));
        let root = points.iter().find(|point| *point == Path::new("/"));
        root.or_else(|| points.iter().find(named)).or(points.first()).cloned()
    }

    fn call<R>(object: &str, interface: &str, method: &str, args: &(impl serde::Serialize + zbus::zvariant::DynamicType)) -> zbus::Result<R>
    where
        R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
    {
        let bus = Connection::system()?;
        bus.call_method(Some(SERVICE), object, Some(interface), method, args)?.body().deserialize()
    }

    fn options() -> HashMap<&'static str, Value<'static>> {
        HashMap::new()
    }

    pub fn mount(object: &str) -> io::Result<PathBuf> {
        call::<String>(object, FILESYSTEM, "Mount", &(options(),)).map(PathBuf::from).map_err(io::Error::other)
    }

    pub fn unmount(object: &str) -> io::Result<()> {
        call::<()>(object, FILESYSTEM, "Unmount", &(options(),)).map_err(io::Error::other)
    }

    pub fn lock(container: &str) -> io::Result<()> {
        call::<()>(container, ENCRYPTED, "Lock", &(options(),)).map_err(io::Error::other)
    }

    /// The keyring's key for a container's passphrase: its UUID, the same on any port.
    fn keyring_key(container: &str) -> Option<String> {
        let reply = Connection::system().ok()?.call_method(Some(SERVICE), container, Some("org.freedesktop.DBus.Properties"), "Get", &(BLOCK, "IdUUID")).ok()?;
        let uuid: String = reply.body().deserialize::<OwnedValue>().ok()?.try_into().ok()?;
        (!uuid.is_empty()).then(|| format!("luks://{uuid}"))
    }

    /// The keyring's passphrase, else asked for (and offered to be kept), again
    /// while it is wrong; then the file system in it mounted.
    pub fn unlock(container: &str, name: &str, ask: &Ask) -> io::Result<PathBuf> {
        let key = keyring_key(container);
        let mut kept = key.as_deref().and_then(crate::secrets::password);
        let mut wrong = "";
        let cleartext = loop {
            let text = format!("{wrong}The passphrase of {name}:");
            let passphrase = match kept.take() {
                Some(passphrase) => passphrase,
                None => match ask(Prompt::Secret { title: "Unlock".into(), text, keyring: key.clone() }) {
                    Reply::Secret(passphrase) => passphrase,
                    _ => return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled")),
                },
            };
            match call::<OwnedObjectPath>(container, ENCRYPTED, "Unlock", &(passphrase, options())) {
                Ok(cleartext) => break cleartext,
                Err(zbus::Error::MethodError(_, Some(message), _)) if message.to_lowercase().contains("passphrase") => wrong = "Wrong passphrase. ",
                Err(err) => return Err(io::Error::other(err)),
            }
        };
        // The file system in it is found a moment after.
        let mut tries = 0;
        loop {
            match mount(cleartext.as_str()) {
                Err(_) if tries < 20 => tries += 1,
                mounted => return mounted,
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod udisks {
    use std::io;
    use std::path::PathBuf;

    pub fn mount(_: &str) -> io::Result<PathBuf> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn unmount(_: &str) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn lock(_: &str) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn unlock(_: &str, _: &str, _: &crate::remote::Ask) -> io::Result<PathBuf> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// As KDE Connect has it: phone, tablet, desktop, laptop or tv.
    pub kind: String,
    pub reachable: bool,
    /// Reachable, and sharing its files.
    pub browsable: bool,
}

/// The paired devices; none where KDE Connect is not running. It is not
/// started for this: D-Bus would start it on the first call.
pub fn paired() -> Vec<Device> {
    // shortcut: a failing bus reads as no devices; the tab hides, as without KDE Connect.
    kde_connect::paired().unwrap_or_default()
}

/// Mounts the device's files: its folders with their names, the main one first.
pub fn mount(id: &str) -> io::Result<Vec<(PathBuf, String)>> {
    let mut folders = main_first(kde_connect::mount(id)?);
    // A mount left from a lost connection fails every read: mount it afresh.
    if let Some((main, _)) = folders.first()
        && !serves(main)
    {
        kde_connect::unmount(id)?;
        folders = main_first(kde_connect::mount(id)?);
        if let Some((main, _)) = folders.first() {
            serves(main);
        }
    }
    match folders.is_empty() {
        true => Err(io::Error::other("the device shares no folder")),
        false => Ok(folders),
    }
}

/// Unmounts the device's files; it stays paired, and the next open mounts them again.
pub fn unmount(id: &str) -> io::Result<()> {
    kde_connect::unmount(id)
}

/// Waits up to 5 s for a fresh mount to serve: KDE Connect answers before
/// sshfs does. False at once where reads fail as a stale mount's do.
fn serves(folder: &Path) -> bool {
    for _ in 0..25 {
        match std::fs::read_dir(folder) {
            Ok(_) => return true,
            // EIO is 5 wherever KDE Connect mounts.
            Err(err) if err.kind() == io::ErrorKind::NotConnected || err.raw_os_error() == Some(5) => return false,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(200)),
        }
    }
    false
}

/// The internal storage holds the camera's folder: the folder holding most
/// others first, then by depth. A card's `/storage/1234-5678` is shallower
/// than Android's `/storage/emulated/0`.
fn main_first(mut folders: Vec<(PathBuf, String)>) -> Vec<(PathBuf, String)> {
    let paths: Vec<PathBuf> = folders.iter().map(|(path, _)| path.clone()).collect();
    let holds = |path: &PathBuf| paths.iter().filter(|other| *other != path && other.starts_with(path)).count();
    folders.sort_by_cached_key(|(path, _)| (std::cmp::Reverse(holds(path)), path.components().count()));
    folders
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod kde_connect {
    use std::collections::HashMap;
    use std::io;
    use std::path::PathBuf;

    use zbus::blocking::Connection;
    use zbus::zvariant::OwnedValue;

    use super::Device;

    const SERVICE: &str = "org.kde.kdeconnect";
    const DEVICE: &str = "org.kde.kdeconnect.device";

    fn call<R>(bus: &Connection, path: &str, interface: &str, method: &str, args: &(impl serde::Serialize + zbus::zvariant::DynamicType)) -> zbus::Result<R>
    where
        R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
    {
        bus.call_method(Some(SERVICE), path, Some(interface), method, args)?.body().deserialize()
    }

    fn property<T: TryFrom<OwnedValue>>(bus: &Connection, path: &str, name: &str) -> Option<T> {
        let value: OwnedValue = call(bus, path, "org.freedesktop.DBus.Properties", "Get", &(DEVICE, name)).ok()?;
        T::try_from(value).ok()
    }

    pub fn paired() -> zbus::Result<Vec<Device>> {
        let bus = Connection::session()?;
        let running: bool = bus
            .call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "NameHasOwner", &SERVICE)?
            .body()
            .deserialize()?;
        if !running {
            return Ok(Vec::new());
        }
        let ids: Vec<String> = call(&bus, "/modules/kdeconnect", "org.kde.kdeconnect.daemon", "devices", &(false, true))?;
        let devices = ids.into_iter().map(|id| {
            let path = format!("/modules/kdeconnect/devices/{id}");
            let reachable = property(&bus, &path, "isReachable").unwrap_or(false);
            let sftp = call(&bus, &path, DEVICE, "hasPlugin", &"kdeconnect_sftp").unwrap_or(false);
            Device {
                name: property(&bus, &path, "name").unwrap_or_else(|| id.clone()),
                kind: property(&bus, &path, "type").unwrap_or_default(),
                id,
                reachable,
                browsable: reachable && sftp,
            }
        });
        Ok(devices.collect())
    }

    pub fn mount(id: &str) -> io::Result<Vec<(PathBuf, String)>> {
        let bus = Connection::session().map_err(io::Error::other)?;
        if !property(&bus, &format!("/modules/kdeconnect/devices/{id}"), "isReachable").unwrap_or(false) {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "not reachable: is KDE Connect open on it, on this network?"));
        }
        let path = format!("/modules/kdeconnect/devices/{id}/sftp");
        let sftp = "org.kde.kdeconnect.device.sftp";
        let mounted: bool = call(&bus, &path, sftp, "mountAndWait", &()).map_err(io::Error::other)?;
        if !mounted {
            let error: String = call(&bus, &path, sftp, "getMountError", &()).unwrap_or_default();
            let error = if error.is_empty() { "could not mount the device".to_owned() } else { error };
            return Err(io::Error::other(error));
        }
        let folders: HashMap<String, OwnedValue> = call(&bus, &path, sftp, "getDirectories", &()).map_err(io::Error::other)?;
        let named = folders.into_iter().map(|(path, name)| (PathBuf::from(path), String::try_from(name).unwrap_or_default()));
        Ok(named.collect())
    }

    pub fn unmount(id: &str) -> io::Result<()> {
        let bus = Connection::session().map_err(io::Error::other)?;
        let path = format!("/modules/kdeconnect/devices/{id}/sftp");
        call(&bus, &path, "org.kde.kdeconnect.device.sftp", "unmount", &()).map_err(io::Error::other)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod kde_connect {
    use std::io;
    use std::path::PathBuf;

    use super::Device;

    pub fn paired() -> Option<Vec<Device>> {
        None
    }

    pub fn mount(_: &str) -> io::Result<Vec<(PathBuf, String)>> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn unmount(_: &str) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn reads_a_disks_size_and_free_space() {
        let (size, free) = disk_space(&std::env::temp_dir()).unwrap();
        assert!(size > 0 && free <= size);
    }

    #[test]
    fn finds_network_shares_but_phones() {
        let mounts = "/dev/nvme0n1p3 / btrfs rw 0 0\n//nas/media /mnt/my\\040media cifs rw 0 0\n\
                      remote: /home/me/Drive fuse.rclone rw 0 0\nkde@phone: /run/user/1000/abc123 fuse.sshfs rw 0 0\n";
        let found = network_mounts(mounts, Some(Path::new("/run/user/1000")));
        let names: Vec<_> = found.iter().map(|volume| (volume.name.as_str(), volume.path.clone().unwrap())).collect();
        assert_eq!(names, [("my media", PathBuf::from("/mnt/my media")), ("Drive", PathBuf::from("/home/me/Drive"))]);
    }

    #[test]
    fn names_gnome_mounts() {
        let dir = std::env::temp_dir().join(format!("pc-gvfs-{}", std::process::id()));
        let mounts = ["smb-share:server=nas,share=media", "mtp:host=SAMSUNG_Galaxy_S23", "google-drive:host=gmail.com,user=me"];
        for name in mounts {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        let mut found = gvfs_mounts(&dir);
        found.sort_by_key(|volume| volume.kind);
        let names: Vec<_> = found.iter().map(|volume| (volume.name.as_str(), volume.kind)).collect();
        let cloud = ("Google Drive (me@gmail.com)", VolumeKind::Cloud);
        assert_eq!(names, [cloud, ("media on nas", VolumeKind::Network), ("SAMSUNG Galaxy S23", VolumeKind::Phone)]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn finds_cloud_folders() {
        let home = std::env::temp_dir().join(format!("pc-cloud-{}", std::process::id()));
        let folders = ["OneDrive - Contoso", "Dropbox", "Boxes", "iCloudDrive", "Library/CloudStorage/GoogleDrive-me@gmail.com"];
        for folder in folders.iter().chain(&["Library/CloudStorage/Box-Box", "Library/Mobile Documents/com~apple~CloudDocs"]) {
            std::fs::create_dir_all(home.join(folder)).unwrap();
        }
        std::fs::write(home.join("Library/CloudStorage/.DS_Store"), b"").unwrap();
        std::fs::create_dir_all(home.join("Library/CloudStorage/.hidden")).unwrap();
        let mut names: Vec<String> = cloud_folders(&home).into_iter().map(|volume| volume.name).collect();
        names.sort();
        let wanted = ["Box", "Dropbox", "Google Drive (me@gmail.com)", "OneDrive - Contoso", "iCloud Drive", "iCloud Drive"];
        assert_eq!(names, wanted);
        std::fs::remove_dir_all(&home).unwrap();
    }

    /// Reads this computer's volumes; run by hand with `--ignored`.
    #[test]
    #[ignore]
    fn lists_volumes() {
        for volume in volumes() {
            println!("{volume:?}");
        }
    }
    /// Unlocks the encrypted volume named PC_LUKS_TEST with PC_LUKS_SECRET, after a wrong
    /// passphrase, writes in it, then unmounts and locks it; run by hand with `--ignored`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn unlocks_an_encrypted_volume() {
        use crate::remote::{Prompt, Reply};
        use std::sync::{Arc, Mutex};
        let (Ok(name), Ok(secret)) = (std::env::var("PC_LUKS_TEST"), std::env::var("PC_LUKS_SECRET")) else {
            return;
        };
        let locked = volumes().into_iter().find(|volume| volume.name == name).expect("listed");
        assert!(locked.encrypted.is_some() && locked.path.is_none() && locked.udisks.is_none(), "{locked:?}");
        // Unlocked, it has the name of the file system in it.
        let find = || volumes().into_iter().find(|volume| volume.encrypted == locked.encrypted).expect("listed");
        let asked = Arc::new(Mutex::new(Vec::new()));
        let heard = asked.clone();
        let ask: Ask = Arc::new(move |prompt| {
            let Prompt::Secret { text, .. } = prompt else { return Reply::Cancel };
            let mut heard = heard.lock().unwrap();
            heard.push(text);
            Reply::Secret(if heard.len() == 1 { "wrong".into() } else { secret.clone() })
        });
        let path = mount_volume(&locked, &ask).unwrap();
        assert!(asked.lock().unwrap()[1].starts_with("Wrong passphrase."), "{asked:?}");
        std::fs::write(path.join("written"), "in it").unwrap();
        let unlocked = find();
        assert_eq!(unlocked.path.as_deref(), Some(path.as_path()), "{unlocked:?}");
        unmount_volume(&unlocked).unwrap();
        let again = find();
        assert!(again.udisks.is_none() && again.path.is_none(), "locked again: {again:?}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_volume_opens_its_main_mount() {
        let points = |list: &[&str]| list.iter().map(std::path::PathBuf::from).collect::<Vec<_>>();
        let main = |list: &[&str], label| super::udisks::main_point(&points(list), label).unwrap();
        assert_eq!(main(&["/swap", "/var/lib/libvirt/qemu/save", "/var/lib/libvirt/vmfast"], Some("VMFast")), std::path::Path::new("/var/lib/libvirt/vmfast"));
        assert_eq!(main(&["/mnt/btr_pool", "/home", "/"], Some("fedora")), std::path::Path::new("/"));
        assert_eq!(main(&["/mnt/a", "/mnt/b"], None), std::path::Path::new("/mnt/a"));
    }

    #[test]
    fn internal_storage_comes_first() {
        let folder = |path: &str| (std::path::PathBuf::from(path), String::new());
        let folders = vec![folder("/m/storage/1234-5678"), folder("/m/storage/emulated/0/DCIM/Camera"), folder("/m/storage/emulated/0")];
        let order: Vec<_> = super::main_first(folders).into_iter().map(|(path, _)| path).collect();
        assert_eq!(order[0], std::path::Path::new("/m/storage/emulated/0"));
        assert_eq!(order[1], std::path::Path::new("/m/storage/1234-5678"));
    }

    /// Reads the session's KDE Connect; run by hand with `--ignored`.
    #[test]
    #[ignore]
    fn lists_paired_devices() {
        for device in super::paired() {
            assert!(!device.id.is_empty() && !device.name.is_empty());
            println!("{device:?}");
        }
    }
}
