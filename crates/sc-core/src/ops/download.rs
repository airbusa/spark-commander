//! A file from a link into this computer's folder: over HTTP(S) with ureq,
//! over FTP(S) with the remote panels' connection. Written under a part name
//! and renamed when whole; a broken download goes on from its part.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ops::{Report, Task};
use crate::remote::{self, Address, Ask, Prompt, Reply, Scheme};
use crate::vfs::{self, Local};

/// Whether `text` is a link this downloads: HTTP(S) or FTP(S).
pub fn is_link(text: &str) -> bool {
    let Some((scheme, rest)) = text.trim().split_once("://") else {
        return false;
    };
    matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https" | "ftp" | "ftps") && !rest.is_empty()
}

/// The link without its password, for titles and the downloaded file's mark.
pub fn shown(link: &str) -> String {
    let link = link.trim();
    let Some((scheme, rest)) = link.split_once("://") else {
        return link.to_owned();
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    match rest[..end].rsplit_once('@') {
        Some((user, host)) => {
            let user = user.split_once(':').map_or(user, |(user, _)| user);
            format!("{scheme}://{user}@{host}{}", &rest[end..])
        }
        None => link.to_owned(),
    }
}

/// The name the link's path ends in, if any.
pub fn link_name(link: &str) -> Option<String> {
    let rest = link.trim().split_once("://")?.1;
    let path = rest.split(['?', '#']).next()?;
    let (_, path) = path.split_once('/')?;
    safe_name(&unescape(path.rsplit('/').next()?))
}

/// One file's name from what a server or link says: the last part of a
/// path in it, without control characters (and Windows' forbidden ones);
/// none when nothing is left of it.
pub fn safe_name(name: &str) -> Option<String> {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = last
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' if cfg!(windows) => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let clean = clean.trim().trim_end_matches('.');
    (!clean.is_empty() && clean != "." && clean != "..").then(|| clean.to_owned())
}

/// The file name a Content-Disposition header gives: `filename*` (UTF-8) first.
fn disposition_name(header: &str) -> Option<String> {
    let parts = header.split(';').map(str::trim);
    let value = |key: &str| parts.clone().find_map(|part| part.split_once('=').filter(|(name, _)| name.trim().eq_ignore_ascii_case(key)).map(|(_, value)| value.trim()));
    if let Some(encoded) = value("filename*") {
        let text = encoded.split_once("''").map_or(encoded, |(_, text)| text);
        if let Some(name) = safe_name(&unescape(text)) {
            return Some(name);
        }
    }
    safe_name(value("filename")?.trim_matches('"'))
}

/// `%20` and such decoded; bad escapes kept as they are.
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes.get(at + 1..at + 3).and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match (bytes[at], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `a (2).txt`, `a (3).txt`, … the first not taken.
fn free_name(target: &Path) -> PathBuf {
    let stem = target.file_stem().unwrap_or_default().to_string_lossy();
    let extension = target.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..).map(|n| target.with_file_name(format!("{stem} ({n}){extension}"))).find(|path| !path.exists()).expect("some number is free")
}

/// Where the download of `link` to `target` is written until whole: beside
/// it, named also by the link, so only this link's goes on from it.
fn part_of(link: &str, target: &Path) -> PathBuf {
    // FNV-1a, as copies' parts: the same name in every build.
    let hash = link.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3));
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{:08x}{}", hash as u32, vfs::PART));
    target.with_file_name(name)
}

/// Downloads `link` into `dir`, as `name` (over a file of that name, which
/// the user agreed to), else as the server or the link names it (beside a
/// file of that name), at most `limit` bytes a second. `ask` answers a
/// server's questions (its password).
pub fn download(link: String, dir: PathBuf, name: Option<String>, limit: Option<u64>, ask: Ask) -> Task {
    let shown = shown(&link);
    let title = match name.clone().or_else(|| link_name(&link)) {
        Some(name) => format!("Download \"{name}\""),
        None => format!("Download {shown}"),
    };
    Task {
        title,
        run: Box::new(move |report| {
            let source = open(&link, ask)?;
            let named = name.is_some();
            let name = name.or(source.name.clone()).or_else(|| link_name(&link)).unwrap_or_else(|| "download".to_owned());
            let mut target = dir.join(&name);
            if !named && target.exists() {
                target = free_name(&target);
            }
            fetch(&link, source, &target, limit, report)?;
            mark(&target, &shown);
            let saved = target.file_name().unwrap_or_default().to_string_lossy().into_owned();
            Ok(format!("saved as {saved}"))
        }),
    }
}

/// A body read from where it starts in the file.
type Body = (Box<dyn Read + Send>, u64);

/// A download's body: the server's, from where it was asked to start.
struct Source {
    reader: Box<dyn Read + Send>,
    /// The whole file's size, if known.
    size: Option<u64>,
    /// Where the body starts in the file.
    from: u64,
    /// What the server names the file.
    name: Option<String>,
    /// Asks for the body from `offset` on, if the server can; else none.
    resume: Box<dyn FnOnce(u64) -> io::Result<Option<Body>> + Send>,
}

fn open(link: &str, ask: Ask) -> io::Result<Source> {
    match link.trim().split_once("://").map(|(scheme, _)| scheme.to_ascii_lowercase()).as_deref() {
        Some("http" | "https") => open_web(link.trim()),
        _ => open_server(link, ask),
    }
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .user_agent("SparkCommander");
    config.build().into()
}

fn open_web(link: &str) -> io::Result<Source> {
    let failed = |err: ureq::Error| io::Error::other(err.to_string());
    let response = agent().get(link).call().map_err(failed)?;
    let header = |name: &str| response.headers().get(name).and_then(|value| value.to_str().ok()).map(str::to_owned);
    let name = header("content-disposition").as_deref().and_then(disposition_name);
    let ranges = header("accept-ranges").is_some_and(|value| value.eq_ignore_ascii_case("bytes"));
    let size = response.body().content_length();
    let link = link.to_owned();
    let resume = move |offset: u64| -> io::Result<Option<Body>> {
        if !ranges {
            return Ok(None);
        }
        let response = agent().get(&link).header("Range", format!("bytes={offset}-")).call().map_err(failed)?;
        // A server that sends it all again is not taken for a part.
        Ok((response.status().as_u16() == 206).then(|| (Box::new(response.into_body().into_reader()) as Box<dyn Read + Send>, offset)))
    };
    Ok(Source { reader: Box::new(response.into_body().into_reader()), size, from: 0, name, resume: Box::new(resume) })
}

/// FTP(S), logged in as the link says (its password, else the keyring's,
/// else asked; anonymous without a user).
fn open_server(link: &str, ask: Ask) -> io::Result<Source> {
    let mut address = Address::parse(link).filter(|address| matches!(address.scheme, Scheme::Ftp | Scheme::Ftps)).ok_or_else(|| io::Error::other("not a link to download"))?;
    let path = address.path.take().filter(|path| path.len() > 1).ok_or_else(|| io::Error::other("the link names no file"))?;
    let path = PathBuf::from(unescape(&path));
    let password = address.user.as_mut().and_then(|user| {
        let (name, password) = user.split_once(':')?;
        let password = unescape(password);
        *user = unescape(name);
        Some(password)
    });
    let given = std::sync::Mutex::new(password);
    let ask: Ask = std::sync::Arc::new(move |prompt| match (&prompt, given.lock().unwrap().take()) {
        (Prompt::Secret { .. }, Some(password)) => Reply::Secret(password),
        _ => ask(prompt),
    });
    let (server, _) = remote::connect(&address, ask)?;
    let meta = server.metadata(&path)?;
    if meta.is_dir {
        return Err(io::Error::other("the link is a folder's"));
    }
    let reader = server.open(&path)?;
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).and_then(|name| safe_name(&name));
    let resume = move |offset: u64| -> io::Result<Option<Body>> {
        match server.open_at(&path, offset) {
            Ok(reader) => Ok(Some((reader, offset))),
            Err(err) if err.kind() == io::ErrorKind::Unsupported => Ok(None),
            Err(err) => Err(err),
        }
    };
    Ok(Source { reader, size: Some(meta.size), from: 0, name, resume: Box::new(resume) })
}

/// The body into `target`'s part, gone on from a part a broken download
/// left; the part renamed to `target` when whole. A cancelled download's
/// part is removed, a failed one's kept.
fn fetch(link: &str, source: Source, target: &Path, limit: Option<u64>, report: &mut Report) -> io::Result<()> {
    let part = part_of(link, target);
    let Source { mut reader, size, mut from, resume, .. } = source;
    let had = std::fs::metadata(&part).map_or(0, |meta| meta.len());
    if had > 0
        && size.is_none_or(|size| had < size)
        && let Some((rest, offset)) = resume(had)?
    {
        (reader, from) = (rest, offset);
    }
    let mut file = match from {
        0 => File::create(&part)?,
        _ => OpenOptions::new().append(true).open(&part)?,
    };
    report.progress.current = target.to_owned();
    report.progress.total_files = 1;
    report.progress.total_bytes = size.unwrap_or(0);
    (report.progress.bytes, report.progress.resumed) = (from, from);
    report.show();
    let limit = limit.filter(|limit| *limit > 0);
    // A tenth of a second's worth at a time under a limit, for an even pace.
    let mut buffer = vec![0; limit.map_or(1 << 16, |limit| (limit / 10).clamp(1024, 1 << 16) as usize)];
    let (start, mut taken) = (std::time::Instant::now(), 0u64);
    let copied = (|| -> io::Result<()> {
        loop {
            report.check()?;
            let read = match reader.read(&mut buffer) {
                Ok(0) => return Ok(()),
                Ok(read) => read,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            };
            file.write_all(&buffer[..read])?;
            taken += read as u64;
            // Ahead of the limit: waits, a little at a time, so a cancel is heard.
            if let Some(limit) = limit {
                let due = Duration::from_secs_f64(taken as f64 / limit as f64);
                while start.elapsed() < due {
                    report.check()?;
                    std::thread::sleep((due - start.elapsed()).min(Duration::from_millis(100)));
                }
            }
            report.progress.bytes += read as u64;
            // A size the server did not tell grows with the file.
            report.progress.total_bytes = size.unwrap_or(report.progress.bytes);
            report.show();
        }
    })();
    drop(file);
    if let Err(err) = copied {
        if report.cancelled() {
            let _ = std::fs::remove_file(&part);
        }
        return Err(err);
    }
    if let Some(size) = size.filter(|size| report.progress.bytes < *size) {
        return Err(io::Error::other(format!("the server sent {} of {size} bytes; downloaded again, it goes on from there", report.progress.bytes)));
    }
    report.progress.files = 1;
    report.show();
    vfs::replace(&Local, &part, target)
}

/// Marks the file as from the internet, as browsers do: Windows warns
/// before running it, macOS asks; Linux keeps where it came from.
fn mark(target: &Path, link: &str) {
    #[cfg(windows)]
    {
        let zone = format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={link}\r\n");
        let _ = std::fs::write(format!("{}:Zone.Identifier", target.display()), zone);
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = std::ffi::CString::new(target.as_os_str().as_bytes()) else {
            return;
        };
        #[cfg(target_os = "macos")]
        {
            let _ = link;
            let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
            let value = format!("0081;{seconds:x};Spark Commander;");
            // SAFETY: both strings are NUL-terminated and outlive the call.
            unsafe { libc::setxattr(path.as_ptr(), c"com.apple.quarantine".as_ptr(), value.as_ptr().cast(), value.len(), 0, 0) };
        }
        #[cfg(target_os = "linux")]
        // SAFETY: both strings are NUL-terminated and outlive the call.
        unsafe {
            libc::setxattr(path.as_ptr(), c"user.xdg.origin.url".as_ptr(), link.as_ptr().cast(), link.len(), 0)
        };
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let _ = (path, link);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_names() {
        assert!(is_link(" https://example.org/a.iso ") && is_link("FTP://host/a"));
        assert!(!is_link("file:///etc/passwd") && !is_link("http://") && !is_link("sftp://host/a") && !is_link("magnet:?xt=urn:btih:abc"));
        assert_eq!(shown("ftp://me:secret@host:21/pub/a.iso"), "ftp://me@host:21/pub/a.iso");
        assert_eq!(shown("https://example.org/a?b=c@d"), "https://example.org/a?b=c@d");
        assert_eq!(link_name("https://example.org/dl/My%20File.tar.gz?x=1#top").as_deref(), Some("My File.tar.gz"));
        assert_eq!((link_name("https://example.org/"), link_name("https://example.org")), (None, None));
        assert_eq!(safe_name("../../.bashrc").as_deref(), Some(".bashrc"));
        assert_eq!((safe_name(".."), safe_name(" . ")), (None, None));
        assert_eq!(safe_name("a\u{7}b.txt").as_deref(), Some("a_b.txt"));
        assert_eq!(disposition_name("attachment; filename=\"plain.zip\"").as_deref(), Some("plain.zip"));
        assert_eq!(disposition_name("attachment; filename=\"x.zip\"; filename*=UTF-8''%C3%A9t%C3%A9.zip").as_deref(), Some("été.zip"));
        assert_eq!(disposition_name("attachment; filename=\"/etc/passwd\"").as_deref(), Some("passwd"));
        assert_eq!(unescape("100%"), "100%");
    }

    /// A server that sends a file in two halves, the first one broken off:
    /// the second download goes on from the part the first left.
    #[test]
    fn downloads_and_goes_on_from_a_part() {
        use std::net::TcpListener;
        let body: Vec<u8> = (0..200_000u32).map(|n| n as u8).collect();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = body.clone();
        std::thread::spawn(move || {
            for (turn, stream) in listener.incoming().enumerate() {
                let mut stream = stream.unwrap();
                let mut request = [0; 2048];
                let read = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..read]).to_string();
                let from = request.to_ascii_lowercase().lines().find_map(|line| line.strip_prefix("range: bytes=").map(str::to_owned)).and_then(|range| range.trim_end_matches('-').parse::<usize>().ok());
                let head = |status: &str, length: usize| format!("HTTP/1.1 {status}\r\nContent-Length: {length}\r\nAccept-Ranges: bytes\r\nContent-Disposition: attachment; filename=\"data.bin\"\r\nConnection: close\r\n\r\n");
                match (turn, from) {
                    // The first download: half the body, then the line drops.
                    (0, _) => {
                        let _ = stream.write_all(head("200 OK", served.len()).as_bytes());
                        let _ = stream.write_all(&served[..100_000]);
                    }
                    (_, Some(from)) => {
                        let _ = stream.write_all(head("206 Partial Content", served.len() - from).as_bytes());
                        let _ = stream.write_all(&served[from..]);
                    }
                    (_, None) => {
                        let _ = stream.write_all(head("200 OK", served.len()).as_bytes());
                        let _ = stream.write_all(&served);
                    }
                }
            }
        });
        let dir = std::env::temp_dir().join(format!("sc-download-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let link = format!("http://127.0.0.1:{port}/get?id=7");
        let ask: Ask = std::sync::Arc::new(|_| Reply::Cancel);
        let queue = crate::ops::Queue::default();
        let run = |queue: &crate::ops::Queue| {
            let id = queue.run_now(download(link.clone(), dir.clone(), None, None, ask.clone()));
            while !queue.jobs().iter().any(|job| job.id == id && job.status.finished()) {
                std::thread::sleep(Duration::from_millis(20));
            }
            queue.jobs().into_iter().find(|job| job.id == id).unwrap()
        };
        let first = run(&queue);
        assert_eq!(first.status, crate::ops::Status::Failed, "{:?}", first.note);
        assert!(!dir.join("data.bin").exists(), "not whole: not in its place");
        let second = run(&queue);
        assert_eq!(second.status, crate::ops::Status::Done, "{:?}", second.note);
        assert_eq!(second.progress.resumed, 100_000);
        assert_eq!(std::fs::read(dir.join("data.bin")).unwrap(), body);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no part left");
        // Again: beside it, never over it.
        let third = run(&queue);
        assert_eq!(third.note.as_deref(), Some("saved as data (2).bin"));
        // At most 500 KB a second: the 200 KB take some 0.4 s.
        let started = std::time::Instant::now();
        let id = queue.run_now(download(link.clone(), dir.clone(), Some("slow.bin".into()), Some(500_000), ask.clone()));
        while !queue.jobs().iter().any(|job| job.id == id && job.status.finished()) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(started.elapsed() >= Duration::from_millis(350), "{:?}", started.elapsed());
        assert_eq!(std::fs::read(dir.join("slow.bin")).unwrap(), body);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
