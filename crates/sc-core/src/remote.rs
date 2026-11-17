//! Servers, whatever the protocol: addresses, what connecting asks, and the
//! connection itself.

pub mod cloud;
pub mod connections;
pub mod ftp;
pub mod mounts;
pub mod secrets;
pub mod sftp;

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::vfs::Remote;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scheme {
    #[default]
    Sftp,
    Ftp,
    /// FTP over TLS, explicit (AUTH TLS) on the FTP port.
    Ftps,
    // The desktop mounts these: see `mounts`.
    Smb,
    Webdav,
    Webdavs,
    Nfs,
    /// Amazon's object storage and those speaking it, through rclone (`cloud`).
    S3,
    /// Backblaze's own, by key ID and application key, through rclone.
    B2,
    /// OpenStack Swift: Rackspace Cloud Files, by user and API key, through rclone.
    Swift,
}

impl Scheme {
    /// Ours, the desktop's, then online storage.
    pub const ALL: [Scheme; 10] = [
        Scheme::Sftp,
        Scheme::Ftp,
        Scheme::Ftps,
        Scheme::Smb,
        Scheme::Webdav,
        Scheme::Webdavs,
        Scheme::Nfs,
        Scheme::S3,
        Scheme::B2,
        Scheme::Swift,
    ];

    /// As the Protocol list and the sidebar show it.
    pub fn label(self) -> &'static str {
        match self {
            Scheme::Sftp => "SFTP",
            Scheme::Ftp => "FTP",
            Scheme::Ftps => "FTPS",
            Scheme::Smb => "SMB",
            Scheme::Webdav => "WebDAV",
            Scheme::Webdavs => "WebDAVS",
            Scheme::Nfs => "NFS",
            Scheme::S3 => "S3",
            Scheme::B2 => "Backblaze B2",
            Scheme::Swift => "Rackspace Cloud Files",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scheme::Sftp => "sftp",
            Scheme::Ftp => "ftp",
            Scheme::Ftps => "ftps",
            Scheme::Smb => "smb",
            Scheme::Webdav => "webdav",
            Scheme::Webdavs => "webdavs",
            Scheme::Nfs => "nfs",
            Scheme::S3 => "s3",
            Scheme::B2 => "b2",
            Scheme::Swift => "swift",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Scheme::Sftp => 22,
            Scheme::Ftp | Scheme::Ftps => 21,
            Scheme::Smb => 445,
            Scheme::Webdav => 80,
            Scheme::Webdavs | Scheme::S3 | Scheme::B2 | Scheme::Swift => 443,
            Scheme::Nfs => 2049,
        }
    }

    /// Not ours: the system mounts it as a folder (KDE's or GNOME's on Linux).
    /// WebDAV on Windows and macOS goes through rclone: theirs are slow and limited.
    pub fn mounted(self) -> bool {
        let desktop = cfg!(any(target_os = "linux", target_os = "freebsd"));
        matches!(self, Scheme::Smb | Scheme::Nfs) || desktop && matches!(self, Scheme::Webdav | Scheme::Webdavs)
    }

    /// Whether it logs in with a user and password; NFS trusts the computer.
    pub fn logs_in(self) -> bool {
        self != Scheme::Nfs
    }
}

/// `sftp://[user@]host[:port][/path]`, or `ftp://`, `ftps://`; S3's
/// `s3://access-key@endpoint/bucket/path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub scheme: Scheme,
    pub user: Option<String>,
    /// For SFTP a `~/.ssh/config` host name, or the server's.
    pub host: String,
    pub port: Option<u16>,
    pub path: Option<String>,
}

impl Address {
    /// Whether both name one server: the scheme, the host and the port (the
    /// scheme's own when none); the user too where both name one.
    pub fn same_server(&self, other: &Address) -> bool {
        let port = |address: &Address| address.port.unwrap_or(address.scheme.default_port());
        let users = match (&self.user, &other.user) {
            (Some(one), Some(other)) => one == other,
            _ => true,
        };
        self.scheme == other.scheme && self.host.eq_ignore_ascii_case(&other.host) && port(self) == port(other) && users
    }

    pub fn parse(text: &str) -> Option<Self> {
        let (scheme, rest) = text.trim().split_once("://")?;
        let scheme = match scheme.to_ascii_lowercase().as_str() {
            "sftp" => Scheme::Sftp,
            "ftp" => Scheme::Ftp,
            "ftps" => Scheme::Ftps,
            "smb" | "cifs" => Scheme::Smb,
            "webdav" | "dav" => Scheme::Webdav,
            "webdavs" | "davs" => Scheme::Webdavs,
            "nfs" => Scheme::Nfs,
            "s3" => Scheme::S3,
            "b2" => Scheme::B2,
            "swift" => Scheme::Swift,
            _ => return None,
        };
        let (authority, path) = match rest.find('/') {
            Some(at) => (&rest[..at], Some(rest[at..].to_owned())),
            None => (rest, None),
        };
        let (user, server) = match authority.rsplit_once('@') {
            Some((user, server)) => (Some(user.to_owned()), server),
            None => (None, authority),
        };
        let (host, port) = match server.rsplit_once(':') {
            Some((host, port)) if port.parse::<u16>().is_ok() => (host, port.parse().ok()),
            _ => (server, None),
        };
        let host = host.trim_start_matches('[').trim_end_matches(']');
        (!host.is_empty()).then(|| Address {
            scheme,
            user: user.filter(|user| !user.is_empty()),
            host: host.to_owned(),
            port,
            path,
        })
    }
}

impl Address {
    /// `smb://user@host:port/path`, as typed.
    pub fn url(&self) -> String {
        let user = self.user.as_ref().map(|user| format!("{user}@")).unwrap_or_default();
        let port = self.port.map(|port| format!(":{port}")).unwrap_or_default();
        let path = self.path.as_deref().unwrap_or("");
        format!("{}://{user}{}{port}{path}", self.scheme.name(), self.host)
    }
}

/// What connecting asks the user.
#[derive(Clone, Debug)]
pub enum Prompt {
    /// The server's key, or TLS certificate, is not known yet.
    UnknownHost {
        host: String,
        key_type: String,
        fingerprint: String,
    },
    /// A password, or a key file's passphrase. A password has the key it
    /// would be kept under in the system keyring.
    Secret {
        title: String,
        text: String,
        keyring: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Trust { remember: bool },
    Secret(String),
    Cancel,
}

/// Answers a prompt; called on a thread of its own, so it may block.
pub type Ask = Arc<dyn Fn(Prompt) -> Reply + Send + Sync>;

/// The key the keyring keeps the server's password under, as the login asks for it.
pub fn keyring_key(address: &Address) -> String {
    match address.scheme {
        Scheme::Sftp => crate::remote::sftp::keyring_key(address),
        scheme => {
            let user = address.user.as_deref().unwrap_or("anonymous");
            let port = address.port.unwrap_or(scheme.default_port());
            secrets::key(scheme.name(), user, &address.host, port)
        }
    }
}

/// Connects and logs in; a password the keyring has is tried before
/// asking. Blocks: run it off the UI thread. Gives the folder to show.
pub fn connect(address: &Address, ask: Ask) -> io::Result<(Arc<dyn Remote>, PathBuf)> {
    let tried = Arc::new(Mutex::new(HashSet::new()));
    let ask: Ask = Arc::new(move |prompt| {
        if let Prompt::Secret {
            keyring: Some(key), ..
        } = &prompt
            && tried.lock().unwrap().insert(key.clone())
            && let Some(password) = secrets::password(key)
        {
            return Reply::Secret(password);
        }
        ask(prompt)
    });
    match address.scheme {
        Scheme::Sftp => {
            let sftp = crate::remote::sftp::Sftp::connect(address, ask)?;
            let start = sftp.start(address);
            Ok((sftp, start))
        }
        Scheme::Ftp | Scheme::Ftps => {
            let ftp = crate::remote::ftp::Ftp::connect(address, ask)?;
            let start = ftp.start(address);
            Ok((ftp, start))
        }
        // `mounts` takes SMB and NFS, and WebDAV on Linux; scmd gets WebDAV here.
        Scheme::Smb | Scheme::Nfs => Err(io::Error::from(io::ErrorKind::Unsupported)),
        Scheme::S3 | Scheme::B2 | Scheme::Swift | Scheme::Webdav | Scheme::Webdavs => {
            let store = crate::remote::cloud::connect(address, &ask)?;
            let start = store.start(address);
            Ok((store, start))
        }
    }
}

/// `ssh` into an SFTP server's `dir`, for the terminal: the program and
/// its arguments; none for other servers.
pub fn ssh_command(url: &str, dir: &std::path::Path) -> Option<(String, Vec<String>)> {
    let address = Address::parse(url).filter(|address| address.scheme == Scheme::Sftp)?;
    let mut args = vec!["-t".to_owned()];
    if let Some(port) = address.port {
        args.extend(["-p".to_owned(), port.to_string()]);
    }
    args.push(match address.user {
        Some(user) => format!("{user}@{}", address.host),
        None => address.host,
    });
    // The server's shell runs it: a folder gone leaves the shell in its home.
    let dir = dir.to_string_lossy().replace('\'', r"'\''");
    args.push(format!("cd '{dir}'; exec \"$SHELL\" -l"));
    Some(("ssh".to_owned(), args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_one_server() {
        let at = |text: &str| Address::parse(text).unwrap();
        assert!(at("sftp://me@NAS.lan:22/srv").same_server(&at("sftp://nas.lan")));
        assert!(at("ftp://nas.lan").same_server(&at("ftp://nas.lan:21/pub")));
        assert!(!at("sftp://me@nas.lan").same_server(&at("sftp://you@nas.lan")));
        assert!(!at("ftp://nas.lan").same_server(&at("ftps://nas.lan")));
        assert!(!at("sftp://nas.lan:2222").same_server(&at("sftp://nas.lan")));
    }

    #[test]
    fn opens_ssh_where_the_panel_is() {
        let (program, args) = ssh_command("sftp://me@example.org:2222", std::path::Path::new("/srv/it's")).unwrap();
        assert_eq!(program, "ssh");
        assert_eq!(args, ["-t", "-p", "2222", "me@example.org", r#"cd '/srv/it'\''s'; exec "$SHELL" -l"#]);
        assert!(ssh_command("ftp://example.org", std::path::Path::new("/")).is_none());
    }

    #[test]
    fn reads_addresses() {
        let address = |text| Address::parse(text);
        assert_eq!(
            address("sftp://me@example.org:2222/var/log"),
            Some(Address {
                scheme: Scheme::Sftp,
                user: Some("me".into()),
                host: "example.org".into(),
                port: Some(2222),
                path: Some("/var/log".into()),
            })
        );
        let alias = address("sftp://myserver").unwrap();
        assert_eq!(
            (alias.user, alias.host.as_str(), alias.port, alias.path),
            (None, "myserver", None, None)
        );
        assert_eq!(address("FTPS://[::1]:21/").unwrap().scheme, Scheme::Ftps);
        assert_eq!(
            address("ftp://anonymous@ftp.example.org").unwrap().scheme,
            Scheme::Ftp
        );
        assert!(address("sftp://").is_none());
        assert!(address("http://example.org").is_none());
        assert!(address("/home").is_none());
    }
}
