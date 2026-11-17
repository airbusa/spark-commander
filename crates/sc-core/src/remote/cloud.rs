//! Online storage through rclone (vendor/rclone): each connection is `rclone
//! serve sftp --stdio`, which our SFTP client speaks. S3 (Amazon's and the
//! stores speaking it: Cloudflare R2, Wasabi, MinIO…), Backblaze B2,
//! Rackspace Cloud Files (Swift), and WebDAV where the system does not mount it.

use std::io::{self, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::remote::{self, Address, Ask, Prompt, Reply, Scheme};
use crate::remote::sftp::Sftp;
use crate::vfs::Fs;

const TRIES: usize = 3;

/// Connects, asking for the secret until the service takes it. Blocks.
pub fn connect(address: &Address, ask: &Ask) -> io::Result<Arc<Sftp>> {
    let program = rclone()?;
    let user = address.user.clone().unwrap_or_default();
    let (backend, kind) = match address.scheme {
        Scheme::B2 => (":b2:", "application_key"),
        Scheme::Swift => (":swift:", "api_key"),
        Scheme::Webdav | Scheme::Webdavs => (":webdav:", "password"),
        _ => (":s3:", "secret_key"),
    };
    // WebDAV with no user: a guest, asked nothing.
    let guest = backend == ":webdav:" && address.user.is_none();
    for attempt in 1..=TRIES {
        let given = match guest {
            true => String::new(),
            false => {
                let host = &address.host;
                let prompt = Prompt::Secret {
                    title: crate::t!("remote.cloud.title", kind = kind),
                    text: match attempt > 1 {
                        true => crate::t!("remote.cloud.ask_again", kind = kind, user = user, host = host),
                        false => crate::t!("remote.cloud.ask", kind = kind, user = user, host = host),
                    },
                    keyring: Some(remote::keyring_key(address)),
                };
                let Reply::Secret(given) = ask(prompt) else {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, crate::t!("remote.cancelled")));
                };
                given
            }
        };
        let mut command = rclone_command(&program);
        // A short directory cache: a refresh shows what changed elsewhere.
        command.args(["serve", "sftp", "--stdio", backend, "--dir-cache-time", "5s", "--poll-interval", "0"]);
        // Secrets go by the environment, which only the user's own programs can read.
        command.envs(options(address, &user, given, &program)?);
        let server = Address { path: None, ..address.clone() };
        // Some say no to the key at the start, S3 once listed.
        let store = Sftp::over_pipe(server.url(), command).and_then(|store| store.list(&store.start(address)).map(|_| store));
        match store {
            Ok(store) => return Ok(store),
            Err(err) if refused(&err) && !guest && attempt < TRIES => continue,
            Err(err) => return Err(err),
        }
    }
    unreachable!("the last try returns")
}

/// The backend's settings, as rclone reads them from the environment.
fn options(address: &Address, user: &str, secret: String, program: &Path) -> io::Result<Vec<(&'static str, String)>> {
    let user = user.to_owned();
    Ok(match address.scheme {
        Scheme::B2 => vec![("RCLONE_B2_ACCOUNT", user), ("RCLONE_B2_KEY", secret)],
        Scheme::Swift => vec![("RCLONE_SWIFT_USER", user), ("RCLONE_SWIFT_KEY", secret), ("RCLONE_SWIFT_AUTH", swift_auth(address))],
        Scheme::Webdav | Scheme::Webdavs => {
            let scheme = if address.scheme == Scheme::Webdavs { "https" } else { "http" };
            let port = address.port.map(|port| format!(":{port}")).unwrap_or_default();
            let mut options = vec![("RCLONE_WEBDAV_URL", format!("{scheme}://{}{port}", host(&address.host)))];
            if !user.is_empty() {
                options.push(("RCLONE_WEBDAV_USER", user));
                options.push(("RCLONE_WEBDAV_PASS", obscure(program, &secret)?));
            }
            options
        }
        _ => {
            let mut options: Vec<_> = s3_options(address).into();
            options.push(("RCLONE_S3_ACCESS_KEY_ID", user));
            options.push(("RCLONE_S3_SECRET_ACCESS_KEY", secret));
            options
        }
    })
}

/// Rackspace's sign-in: its identity service is v2.0, the older auth.api… v1.0.
fn swift_auth(address: &Address) -> String {
    let version = if address.host.starts_with("identity.") { "v2.0" } else { "v1.0" };
    let port = address.port.map(|port| format!(":{port}")).unwrap_or_default();
    format!("https://{}{port}/{version}", host(&address.host))
}

/// An IPv6 address in brackets, as URLs have them.
fn host(host: &str) -> String {
    if host.contains(':') { format!("[{host}]") } else { host.to_owned() }
}

/// The password as rclone's WebDAV wants it, obscured; by its pipe, never on a command line.
fn obscure(program: &Path, password: &str) -> io::Result<String> {
    let mut obscure = rclone_command(program);
    let mut child = obscure.args(["obscure", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    child.stdin.take().expect("piped").write_all(password.as_bytes())?;
    let out = child.wait_with_output()?;
    let obscured = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    match out.status.success() && !obscured.is_empty() {
        true => Ok(obscured),
        false => Err(io::Error::other(crate::t!("remote.cloud.obscure_failed"))),
    }
}

/// rclone with no config file of the user's, asking nothing on its pipe.
fn rclone_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.args(["--config", "", "--ask-password=false"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Ours (the add-on, vendor/rclone, or `rclone/` beside the app), never the system's.
fn rclone() -> io::Result<PathBuf> {
    let ours = crate::addons::bundled::path("rclone", if cfg!(windows) { "rclone.exe" } else { "rclone" });
    match ours.is_file() {
        true => Ok(ours),
        false => Err(io::Error::new(io::ErrorKind::NotFound, crate::t!("remote.cloud.rclone_missing"))),
    }
}

/// A wrong key, secret or password, as S3, B2, Swift and WebDAV say it.
fn refused(err: &io::Error) -> bool {
    let text = err.to_string().to_lowercase();
    ["invalidaccesskeyid", "signaturedoesnotmatch", "401 unauthorized", "authorization failed", "failed to authorize account"].iter().any(|code| text.contains(code))
}

/// The endpoint's store, region and URL, as rclone's S3 takes them.
fn s3_options(address: &Address) -> [(&'static str, String); 3] {
    let host = address.host.to_lowercase();
    let provider = match () {
        _ if host.ends_with(".amazonaws.com") => "AWS",
        _ if host.ends_with(".r2.cloudflarestorage.com") => "Cloudflare",
        _ if host.ends_with(".wasabisys.com") => "Wasabi",
        // s3j8.fra.idrivee2-16.com: it signs with no region.
        _ if host.contains(".idrivee2-") && host.ends_with(".com") => "IDrive",
        _ => "Other",
    };
    let labels: Vec<&str> = host.split('.').collect();
    // s3.eu-central-1.amazonaws.com, s3.us-west-004.backblazeb2.com: the region comes second.
    let region = match labels.as_slice() {
        _ if provider == "Cloudflare" => "auto",
        ["s3", region, _, _, ..] => region,
        _ => "us-east-1",
    };
    // shortcut: a store on this computer or the local network is plain HTTP, as MinIO's usually is; a setting when one has TLS.
    let local = host == "localhost"
        || host.parse::<IpAddr>().is_ok_and(|ip| match ip {
            IpAddr::V4(ip) => ip.is_loopback() || ip.is_private(),
            IpAddr::V6(ip) => ip.is_loopback(),
        });
    let scheme = if local { "http" } else { "https" };
    let server = self::host(&host);
    let port = address.port.map(|port| format!(":{port}")).unwrap_or_default();
    [
        ("RCLONE_S3_PROVIDER", provider.to_owned()),
        ("RCLONE_S3_REGION", region.to_owned()),
        ("RCLONE_S3_ENDPOINT", format!("{scheme}://{server}{port}")),
    ]
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;
    use crate::vfs::Remote;

    #[test]
    fn finds_the_store() {
        let options = |url: &str| s3_options(&Address::parse(url).unwrap()).map(|(_, value)| value);
        let aws = options("s3://AK@s3.eu-central-1.amazonaws.com/bucket");
        assert_eq!(aws, ["AWS", "eu-central-1", "https://s3.eu-central-1.amazonaws.com"]);
        let b2 = options("s3://AK@s3.us-west-004.backblazeb2.com");
        assert_eq!(b2, ["Other", "us-west-004", "https://s3.us-west-004.backblazeb2.com"]);
        let r2 = options("s3://AK@abc.r2.cloudflarestorage.com");
        assert_eq!(r2, ["Cloudflare", "auto", "https://abc.r2.cloudflarestorage.com"]);
        let idrive = options("s3://AK@s3j8.fra.idrivee2-16.com/backup");
        assert_eq!(idrive, ["IDrive", "us-east-1", "https://s3j8.fra.idrivee2-16.com"]);
        let minio = options("s3://AK@192.168.1.5:9000");
        assert_eq!(minio, ["Other", "us-east-1", "http://192.168.1.5:9000"]);
    }

    /// `rclone serve webdav --user tester --pass s3cret` on a folder with
    /// `docs/`, as Windows and macOS reach WebDAV: SC_DAV_TEST=<its port>.
    #[test]
    #[ignore]
    fn talks_webdav() {
        let port = std::env::var("SC_DAV_TEST").unwrap();
        let address = Address::parse(&format!("webdav://tester@127.0.0.1:{port}/docs")).unwrap();
        let tries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = tries.clone();
        let ask: Ask = Arc::new(move |_| {
            let first = counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0;
            Reply::Secret(if first { "wrong".into() } else { "s3cret".into() })
        });
        let dav = connect(&address, &ask).unwrap();
        assert_eq!(tries.load(std::sync::atomic::Ordering::Relaxed), 2, "a wrong password is asked again");
        let dir = dav.start(&address);
        assert_eq!(dir, Path::new("/docs"));
        dav.create(&dir.join("new.txt")).unwrap().write_all(b"dav").unwrap();
        let names: Vec<String> = dav.entries(&dir).unwrap().iter().map(|entry| entry.display_name()).collect();
        assert!(names.contains(&"new.txt".to_owned()), "{names:?}");
        dav.remove(&dir.join("new.txt"), false).unwrap();
    }

    /// `rclone serve s3 --auth-key AKIATESTKEY0001,SECRETKEY0000000000001` as
    /// the store, with a folder `bucket`: SC_S3_TEST=<its port>.
    #[test]
    #[ignore]
    fn talks_to_a_store() {
        let port = std::env::var("SC_S3_TEST").unwrap();
        let address = Address::parse(&format!("s3://AKIATESTKEY0001@127.0.0.1:{port}/bucket")).unwrap();
        let tries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = tries.clone();
        let ask: Ask = Arc::new(move |_| {
            let first = counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0;
            Reply::Secret(if first { "wrong".into() } else { "SECRETKEY0000000000001".into() })
        });
        let store = connect(&address, &ask).unwrap();
        assert_eq!(tries.load(std::sync::atomic::Ordering::Relaxed), 2);
        let dir = store.start(&address);
        let mut file = store.create(&dir.join("made.txt")).unwrap();
        file.write_all(b"made").unwrap();
        drop(file);
        let mut text = String::new();
        store.open(&dir.join("made.txt")).unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(text, "made");
        store.rename(&dir.join("made.txt"), &dir.join("moved.txt")).unwrap();
        let names: Vec<String> = store.entries(&dir).unwrap().iter().map(|entry| entry.display_name()).collect();
        assert!(names.contains(&"moved.txt".to_owned()), "{names:?}");
        store.remove(&dir.join("moved.txt"), false).unwrap();
    }
}
