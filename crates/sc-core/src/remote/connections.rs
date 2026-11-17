//! Saved servers, and the hosts `~/.ssh/config` names.

use std::io;

use serde::{Deserialize, Serialize};

use crate::remote::{self, Address, Scheme};
use crate::{remote::secrets, settings};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    pub name: String,
    pub scheme: Scheme,
    pub host: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    /// The folder it starts in; the login folder if none.
    pub path: Option<String>,
    /// The list's folder it is in; none: at the top.
    pub folder: Option<String>,
}

impl Connection {
    pub fn address(&self) -> Address {
        let (host, port) = bare_host(&self.host);
        Address {
            scheme: self.scheme,
            user: self.user.clone().filter(|user| !user.is_empty()),
            host,
            port: self.port.or(port),
            path: self.path.clone().filter(|path| path.starts_with('/')),
        }
    }

    /// `sftp://user@host:2222/path`, as the path field takes it.
    pub fn url(&self) -> String {
        self.address().url()
    }
}

/// Kept in `connections.json`; passwords are in the system keyring.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Connections {
    /// In the list's order within each folder.
    pub saved: Vec<Connection>,
    /// The list's folders in their order, empty ones too.
    pub folders: Vec<String>,
}

impl Connections {
    pub fn load() -> io::Result<Self> {
        let mut connections: Self = settings::read_json("connections.json")?;
        // A folder named only by its connections (the file written by hand) is listed too.
        for folder in connections.saved.iter().filter_map(|connection| connection.folder.clone()) {
            if !connections.folders.contains(&folder) {
                connections.folders.push(folder);
            }
        }
        Ok(connections)
    }

    pub fn save(&self) -> io::Result<()> {
        settings::write_json("connections.json", self)
    }

    /// Saved connection `from` into `folder`, before `before` (one there), else
    /// last in it; its place after.
    pub fn move_connection(&mut self, from: usize, folder: Option<String>, before: Option<usize>) -> Option<usize> {
        if self.saved.get(from)?.folder == folder && before == Some(from) {
            return Some(from);
        }
        let mut moved = self.saved.remove(from);
        moved.folder = folder;
        let before = before.filter(|&at| at != from).map(|at| if at > from { at - 1 } else { at });
        let there = |at: &usize| self.saved.get(*at).is_some_and(|other| other.folder == moved.folder);
        let at = match before.filter(there) {
            Some(at) => at,
            None => self.saved.iter().rposition(|other| other.folder == moved.folder).map_or(self.saved.len(), |last| last + 1),
        };
        self.saved.insert(at, moved);
        Some(at)
    }

    /// Saved connection `ix` a place up or down in its folder; its place after.
    pub fn step_connection(&mut self, ix: usize, up: bool) -> Option<usize> {
        let folder = self.saved.get(ix)?.folder.clone();
        let mut same = (0..self.saved.len()).filter(|&at| self.saved[at].folder == folder);
        let other = match up {
            true => same.rfind(|&at| at < ix),
            false => same.find(|&at| at > ix),
        }?;
        self.saved.swap(ix, other);
        Some(other)
    }

    /// Folder `name` before `before`, else last.
    pub fn move_folder(&mut self, name: &str, before: Option<&str>) {
        let Some(from) = self.folders.iter().position(|folder| folder == name) else {
            return;
        };
        if before == Some(name) {
            return;
        }
        let moved = self.folders.remove(from);
        let at = before.and_then(|before| self.folders.iter().position(|folder| folder == before));
        self.folders.insert(at.unwrap_or(self.folders.len()), moved);
    }

    /// Folder `name` a place up or down; whether it moved.
    pub fn step_folder(&mut self, name: &str, up: bool) -> bool {
        let Some(ix) = self.folders.iter().position(|folder| folder == name) else {
            return false;
        };
        let other = if up { ix.checked_sub(1) } else { Some(ix + 1).filter(|&at| at < self.folders.len()) };
        other.inspect(|&other| self.folders.swap(ix, other)).is_some()
    }

    /// A folder named `new`, or `old` renamed to it; false when the name is
    /// empty or another folder's.
    pub fn name_folder(&mut self, old: Option<&str>, new: &str) -> bool {
        let new = new.trim();
        if new.is_empty() || old != Some(new) && self.folders.iter().any(|folder| folder == new) {
            return false;
        }
        match old {
            None => self.folders.push(new.to_owned()),
            Some(old) => {
                self.folders.iter_mut().filter(|folder| *folder == old).for_each(|folder| *folder = new.to_owned());
                let inside = self.saved.iter_mut().filter(|connection| connection.folder.as_deref() == Some(old));
                inside.for_each(|connection| connection.folder = Some(new.to_owned()));
            }
        }
        true
    }

    /// Folder `name` gone with its connections; those removed.
    pub fn remove_folder(&mut self, name: &str) -> Vec<Connection> {
        self.folders.retain(|folder| folder != name);
        let (gone, kept) = std::mem::take(&mut self.saved).into_iter().partition(|connection| connection.folder.as_deref() == Some(name));
        self.saved = kept;
        gone
    }
}

/// After `gone` left the saved: its password too, unless another logs in with it.
pub fn forget_password(saved: &[Connection], gone: &Connection) {
    let key = remote::keyring_key(&gone.address());
    if !saved.iter().any(|kept| remote::keyring_key(&kept.address()) == key) {
        std::thread::spawn(move || secrets::forget(&key));
    }
}

/// The `Host` names of `~/.ssh/config`, without patterns.
pub fn ssh_hosts() -> Vec<String> {
    let home = std::env::home_dir().unwrap_or_default();
    let text = std::fs::read_to_string(home.join(".ssh").join("config")).unwrap_or_default();
    hosts_in(&text)
}

fn hosts_in(text: &str) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if !words
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("host"))
        {
            continue;
        }
        for host in words.filter(|host| !host.contains(['*', '?', '!'])) {
            if !hosts.iter().any(|known| known == host) {
                hosts.push(host.to_owned());
            }
        }
    }
    hosts
}

/// The server as typed, an endpoint's URL pasted whole too (`https://s3.example.com:9000/`):
/// its scheme and path dropped, its port apart.
fn bare_host(typed: &str) -> (String, Option<u16>) {
    let typed = typed.trim();
    let host = typed.split_once("://").map_or(typed, |(_, rest)| rest);
    let host = host.split('/').next().unwrap_or_default();
    // One colon is a port; more are IPv6's own.
    match host.split_once(':') {
        Some((name, port)) if !port.contains(':') => (name.to_owned(), port.parse().ok()),
        _ => (host.trim_matches(['[', ']']).to_owned(), None),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn takes_a_pasted_endpoint() {
        let bare = |typed: &str| super::bare_host(typed);
        assert_eq!(bare("https://s3j8.fra.idrivee2-16.com"), ("s3j8.fra.idrivee2-16.com".into(), None));
        assert_eq!(bare(" http://minio.lan:9000/bucket/ "), ("minio.lan".into(), Some(9000)));
        assert_eq!(bare("files.example.com"), ("files.example.com".into(), None));
        assert_eq!(bare("fe80::1"), ("fe80::1".into(), None));
    }

    use super::*;

    #[test]
    fn names_servers() {
        let config = "Host *\n  ServerAliveInterval 30\nHost web web2 !bad\n  HostName example.org\nhost db?\nHOST backup\n";
        assert_eq!(hosts_in(config), ["web", "web2", "backup"]);
        let saved = Connection {
            name: "Mine".into(),
            scheme: Scheme::Ftps,
            host: " ftp.example.org ".into(),
            port: Some(990),
            user: Some("me".into()),
            path: Some("/pub".into()),
            folder: None,
        };
        assert_eq!(saved.url(), "ftps://me@ftp.example.org:990/pub");
        assert_eq!(Address::parse(&saved.url()), Some(saved.address()));
        let plain = Connection {
            host: "web".into(),
            ..Connection::default()
        };
        assert_eq!(plain.url(), "sftp://web");
    }

    #[test]
    fn folders_and_order() {
        let named = |name: &str, folder: Option<&str>| Connection { name: name.into(), folder: folder.map(Into::into), ..Connection::default() };
        let mut all = Connections { saved: vec![named("a", None), named("b", Some("Work")), named("c", None), named("d", Some("Work"))], folders: vec!["Work".into()] };
        let names = |all: &Connections| all.saved.iter().map(|c| format!("{}{}", c.name, c.folder.as_deref().unwrap_or(""))).collect::<Vec<_>>();
        // Into a folder before one of it; last into the top.
        assert_eq!(all.move_connection(0, Some("Work".into()), Some(3)), Some(2));
        assert_eq!(names(&all), ["bWork", "c", "aWork", "dWork"]);
        assert_eq!(all.move_connection(3, None, None), Some(2));
        assert_eq!(names(&all), ["bWork", "c", "d", "aWork"]);
        assert_eq!(all.move_connection(1, None, Some(1)), Some(1), "onto itself: stays");
        // Up past the top level's, among its folder's only.
        assert_eq!(all.step_connection(3, true), Some(0));
        assert_eq!(names(&all), ["aWork", "c", "d", "bWork"]);
        assert_eq!(all.step_connection(0, true), None);
        assert!(all.name_folder(None, "Home") && !all.name_folder(None, " Home ") && !all.name_folder(None, ""));
        assert!(all.step_folder("Home", true) && !all.step_folder("Home", true));
        all.move_folder("Home", None);
        assert_eq!(all.folders, ["Work", "Home"]);
        assert!(all.name_folder(Some("Work"), "Office"));
        assert_eq!(names(&all), ["aOffice", "c", "d", "bOffice"]);
        assert_eq!(all.remove_folder("Office").len(), 2);
        assert_eq!((names(&all), all.folders.clone()), (vec!["c".to_owned(), "d".to_owned()], vec!["Home".to_owned()]));
    }
}
