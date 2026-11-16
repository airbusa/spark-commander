//! Commands > Synchronize Dirs, as Total Commander's: two folder trees compared
//! by name, size and time, or content; what is copied which way, or deleted,
//! to make them alike, run as one job of the queue.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::Entry;
use crate::ops::{Report, Task};
use crate::vfs::{self, Fs, Remote};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub subfolders: bool,
    /// Files of the same size are read, not their times trusted.
    pub by_content: bool,
    pub ignore_date: bool,
    /// The right side becomes the left's copy: what differs goes right, what only it has is deleted.
    pub asymmetric: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { subfolders: true, by_content: false, ignore_date: false, asymmetric: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Equal,
    /// Not alike, and neither is newer: the same time and another size, or a file and a folder.
    Different,
    LeftNewer,
    RightNewer,
    LeftOnly,
    RightOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Nothing,
    ToRight,
    ToLeft,
    DeleteLeft,
    DeleteRight,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Side {
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Debug)]
pub struct Item {
    /// The same below both folders.
    pub path: PathBuf,
    pub left: Option<Side>,
    pub right: Option<Side>,
    pub state: State,
    pub action: Action,
}

impl Item {
    /// What a click on its action gives in turn.
    pub fn choices(&self) -> &'static [Action] {
        match (self.left.is_some(), self.right.is_some()) {
            (true, true) => &[Action::ToRight, Action::ToLeft, Action::Nothing],
            (true, false) => &[Action::ToRight, Action::DeleteLeft, Action::Nothing],
            _ => &[Action::ToLeft, Action::DeleteRight, Action::Nothing],
        }
    }
}

/// A side: its file system, the server's when it is one's, and the folder in it.
#[derive(Clone)]
pub struct Root {
    pub fs: Arc<dyn Fs>,
    pub remote: Option<Arc<dyn Remote>>,
    pub dir: PathBuf,
}

impl Root {
    pub fn new(fs: Arc<dyn Fs>, remote: Option<Arc<dyn Remote>>, dir: PathBuf) -> Self {
        Self { fs, remote, dir }
    }

    fn entries(&self, dir: &Path) -> io::Result<Vec<Entry>> {
        match &self.remote {
            Some(remote) => remote.entries(dir),
            None => crate::read_dir(dir),
        }
    }
}

/// Times this close are the same: FAT keeps two seconds, servers whole ones.
const SLACK: Duration = Duration::from_secs(2);

/// Every file and folder that is on either side, with what to do about it;
/// files by `mask` (empty: all). `at` hears of each folder looked into.
pub fn compare(left: &Root, right: &Root, options: Options, mask: &str, stop: &AtomicBool, at: &mut dyn FnMut(&Path)) -> io::Result<Vec<Item>> {
    let walk = Walk { left, right, options, mask: mask.trim(), stop };
    let mut items = Vec::new();
    walk.folder(Path::new(""), (true, true), at, &mut items)?;
    Ok(items)
}

struct Walk<'a> {
    left: &'a Root,
    right: &'a Root,
    options: Options,
    mask: &'a str,
    stop: &'a AtomicBool,
}

impl Walk<'_> {
    fn all(&self) -> bool {
        matches!(self.mask, "" | "*" | "*.*")
    }

    fn wanted(&self, name: &OsString) -> bool {
        self.all() || crate::associations::matches(self.mask, &name.to_string_lossy())
    }

    /// `rel`'s entries on the sides that have it.
    fn folder(&self, rel: &Path, sides: (bool, bool), at: &mut dyn FnMut(&Path), items: &mut Vec<Item>) -> io::Result<()> {
        if self.stop.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "stopped"));
        }
        at(rel);
        let left = if sides.0 { listing(self.left, rel)? } else { Vec::new() };
        let right = if sides.1 { listing(self.right, rel)? } else { Vec::new() };
        let names: BTreeSet<&OsString> = left.iter().chain(&right).map(|entry| &entry.name).collect();
        for name in names {
            let find = |list: &[Entry]| list.iter().find(|entry| entry.name == *name).cloned();
            let (l, r) = (find(&left), find(&right));
            let path = rel.join(name);
            let dir = l.as_ref().or(r.as_ref()).is_some_and(|entry| entry.is_dir);
            match (&l, &r) {
                (Some(l), Some(r)) if l.is_dir && r.is_dir => {
                    if self.options.subfolders && !self.link(&path) {
                        self.folder(&path, (true, true), at, items)?;
                    }
                }
                (Some(l), Some(r)) if l.is_dir != r.is_dir => items.push(self.item(path, Some(l), Some(r), State::Different)),
                (Some(l), Some(r)) => {
                    if self.wanted(name) {
                        let state = self.files(&path, l, r)?;
                        items.push(self.item(path, Some(l), Some(r), state));
                    }
                }
                // A folder on one side: whole, or its files the mask wants.
                (Some(_), None) | (None, Some(_)) if dir => {
                    if !self.options.subfolders {
                        continue;
                    }
                    match self.all() {
                        true => items.push(self.item(path, l.as_ref(), r.as_ref(), if l.is_some() { State::LeftOnly } else { State::RightOnly })),
                        false => self.folder(&path, (l.is_some(), r.is_some()), at, items)?,
                    }
                }
                (Some(l), None) if self.wanted(name) => items.push(self.item(path, Some(l), None, State::LeftOnly)),
                (None, Some(r)) if self.wanted(name) => items.push(self.item(path, None, Some(r), State::RightOnly)),
                _ => {}
            }
        }
        Ok(())
    }

    /// A folder that is a link on either side is not gone into: it may lead back.
    fn link(&self, rel: &Path) -> bool {
        [self.left, self.right].iter().any(|root| root.fs.metadata(&root.dir.join(rel)).is_ok_and(|meta| meta.is_link))
    }

    fn files(&self, rel: &Path, l: &Entry, r: &Entry) -> io::Result<State> {
        let timed = !self.options.ignore_date;
        let newer = |a: Option<SystemTime>, b: Option<SystemTime>| timed && a.zip(b).and_then(|(a, b)| a.duration_since(b).ok()).is_some_and(|by| by > SLACK);
        let (left_newer, right_newer) = (newer(l.modified, r.modified), newer(r.modified, l.modified));
        let same = || -> io::Result<bool> { Ok(l.size == r.size && (!self.options.by_content || self.same_content(rel)?)) };
        // Times alike, or aside: the size, or the contents, tell.
        if !(left_newer || right_newer) {
            return Ok(if same()? { State::Equal } else { State::Different });
        }
        // One is newer, unless reading them finds them the same.
        if self.options.by_content && same()? {
            return Ok(State::Equal);
        }
        Ok(if left_newer { State::LeftNewer } else { State::RightNewer })
    }

    fn same_content(&self, rel: &Path) -> io::Result<bool> {
        let mut a = self.left.fs.open(&self.left.dir.join(rel))?;
        let mut b = self.right.fs.open(&self.right.dir.join(rel))?;
        let (mut x, mut y) = (vec![0; 1 << 16], vec![0; 1 << 16]);
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "stopped"));
            }
            let read = fill(&mut a, &mut x)?;
            if read != fill(&mut b, &mut y)? || x[..read] != y[..read] {
                return Ok(false);
            }
            if read == 0 {
                return Ok(true);
            }
        }
    }

    fn item(&self, path: PathBuf, l: Option<&Entry>, r: Option<&Entry>, state: State) -> Item {
        let mirror = self.options.asymmetric;
        let action = match state {
            State::Equal => Action::Nothing,
            State::Different if mirror => Action::ToRight,
            State::Different => Action::Nothing,
            State::LeftNewer | State::LeftOnly => Action::ToRight,
            State::RightNewer if mirror => Action::ToRight,
            State::RightNewer => Action::ToLeft,
            State::RightOnly if mirror => Action::DeleteRight,
            State::RightOnly => Action::ToLeft,
        };
        let side = |entry: &Entry| Side { is_dir: entry.is_dir, size: entry.size, modified: entry.modified };
        Item { path, left: l.map(side), right: r.map(side), state, action }
    }
}

fn listing(root: &Root, rel: &Path) -> io::Result<Vec<Entry>> {
    let entries = root.entries(&root.dir.join(rel))?;
    Ok(entries.into_iter().filter(|entry| entry.name != crate::PARENT).collect())
}

/// Reads until `buffer` is full or the file ends.
fn fill(file: &mut dyn Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut read = 0;
    while read < buffer.len() {
        match file.read(&mut buffer[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
    Ok(read)
}

/// The queue's job doing `items`' actions: copies replace what is there,
/// deletions here go to the trash. One that fails is left, the rest done.
pub fn synchronize(left: Root, right: Root, items: Vec<Item>) -> Task {
    let items: Vec<Item> = items.into_iter().filter(|item| item.action != Action::Nothing).collect();
    let title = format!("Synchronize {} and {}", place(&left), place(&right));
    Task {
        title,
        run: Box::new(move |report| {
            report.progress.total_files = items.len() as u64;
            // Folders' contents are counted as they are copied.
            let copied = items.iter().filter(|item| matches!(item.action, Action::ToRight | Action::ToLeft));
            let source = |item: &&Item| if item.action == Action::ToRight { item.left.clone() } else { item.right.clone() };
            report.progress.total_bytes = copied.filter_map(|item| source(&item)).filter(|side| !side.is_dir).map(|side| side.size).sum();
            let (mut failed, mut first) = (0, None);
            for item in &items {
                report.check()?;
                report.progress.current = item.path.clone();
                report.show();
                let fresh = match item.action {
                    Action::ToRight | Action::ToLeft => unchanged(&left, item, &item.left) && unchanged(&right, item, &item.right),
                    Action::DeleteLeft => unchanged(&left, item, &item.left),
                    Action::DeleteRight => unchanged(&right, item, &item.right),
                    Action::Nothing => true,
                };
                let done = match item.action {
                    _ if !fresh => Err(io::Error::other("changed since the comparison; compare again")),
                    Action::ToRight => copy(&left, &right, &item.path, report),
                    Action::ToLeft => copy(&right, &left, &item.path, report),
                    Action::DeleteLeft => delete(&left, &item.path),
                    Action::DeleteRight => delete(&right, &item.path),
                    Action::Nothing => Ok(()),
                };
                match done {
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => return Err(err),
                    Err(err) => {
                        failed += 1;
                        report.progress.skipped += 1;
                        first.get_or_insert_with(|| format!("{}: {err}", item.path.display()));
                    }
                    Ok(()) => {}
                }
                report.progress.files += 1;
                report.show();
            }
            match first {
                Some(first) => Err(io::Error::other(format!("{failed} not synchronized; {first}"))),
                None => Ok(format!("{} synchronized", items.len())),
            }
        }),
    }
}

/// Whether `item` on `root` is still as compared (`side`): nothing there, or
/// a folder or file of that time, and a file of that size; in whole seconds,
/// as servers keep times. A link only still there.
// shortcut: a folder's time tells only of changes right in it, not deeper; compare its tree if deletes must know.
fn unchanged(root: &Root, item: &Item, side: &Option<Side>) -> bool {
    let now = root.fs.metadata(&root.dir.join(&item.path)).ok();
    let seconds = |time: Option<SystemTime>| time.and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|since| since.as_secs());
    match (side, now) {
        (None, None) => true,
        (Some(_), Some(now)) if now.is_link => true,
        (Some(was), Some(now)) => was.is_dir == now.is_dir && seconds(was.modified) == seconds(now.modified) && (was.is_dir || was.size == now.size),
        _ => false,
    }
}

/// Where a side is, as the path bar says it.
pub fn place(root: &Root) -> String {
    match &root.remote {
        None => root.dir.display().to_string(),
        Some(remote) => format!("{}{}", remote.url(), root.dir.display()),
    }
}

/// `rel` from one side to the other, a folder with all in it; its folders made first.
fn copy(from: &Root, to: &Root, rel: &Path, report: &mut Report) -> io::Result<()> {
    let (source, target) = (from.dir.join(rel), to.dir.join(rel));
    if let Some(parent) = target.parent() {
        vfs::create_all(&*to.fs, parent)?;
    }
    copy_tree(from, &source, &*to.fs, &target, report)
}

fn copy_tree(root: &Root, source: &Path, to: &dyn Fs, target: &Path, report: &mut Report) -> io::Result<()> {
    let from = &*root.fs;
    let meta = from.metadata(source)?;
    // A link as a link on its own file system; to another, what it leads to.
    if meta.is_link && from.id() == to.id() {
        if to.metadata(target).is_ok() {
            vfs::remove_all(to, target)?;
        }
        return from.copy_link(source, target);
    }
    if !meta.is_dir {
        return vfs::copy_file(from, source, to, target, &meta, &mut |read, _| {
            report.progress.bytes += read;
            report.progress.total_bytes = report.progress.total_bytes.max(report.progress.bytes);
            report.show();
            report.check()
        });
    }
    if to.metadata(target).is_err() {
        to.mkdir(target)?;
    }
    for entry in root.entries(source)? {
        if entry.name != crate::PARENT {
            copy_tree(root, &source.join(&entry.name), to, &target.join(&entry.name), report)?;
        }
    }
    let _ = to.set_meta(target, &meta);
    Ok(())
}

fn delete(root: &Root, rel: &Path) -> io::Result<()> {
    let path = root.dir.join(rel);
    match root.fs.id() == "local" {
        true => crate::ops::undo::to_trash(&path),
        false => vfs::remove_all(&*root.fs, &path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::Local;
    use std::fs;

    fn touch(path: &Path, text: &str, age: u64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        let when = SystemTime::now() - Duration::from_secs(age);
        fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
    }

    #[test]
    fn compares_two_trees_and_makes_them_alike() {
        let base = std::env::temp_dir().join(format!("sc-sync-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (a, b) = (base.join("a"), base.join("b"));
        touch(&a.join("same.txt"), "same", 100);
        touch(&b.join("same.txt"), "same", 100);
        touch(&a.join("newer.txt"), "new", 10);
        touch(&b.join("newer.txt"), "old", 1000);
        touch(&a.join("sub/only-left.txt"), "l", 50);
        touch(&b.join("only-right/x.txt"), "r", 50);
        touch(&a.join("sized.txt"), "one", 100);
        touch(&b.join("sized.txt"), "three", 100);
        touch(&a.join("note.md"), "md", 100);
        fs::create_dir_all(b.join("sub")).unwrap();
        let root = |dir: &Path| Root::new(Arc::new(Local), None, dir.to_owned());
        let (left, right) = (root(&a), root(&b));
        let stop = AtomicBool::new(false);
        let found = compare(&left, &right, Options::default(), "", &stop, &mut |_| {}).unwrap();
        let of = |name: &str| found.iter().find(|item| item.path == Path::new(name)).map(|item| (item.state, item.action));
        assert_eq!(of("same.txt"), Some((State::Equal, Action::Nothing)));
        assert_eq!(of("newer.txt"), Some((State::LeftNewer, Action::ToRight)));
        assert_eq!(of("sub/only-left.txt"), Some((State::LeftOnly, Action::ToRight)));
        assert_eq!(of("only-right"), Some((State::RightOnly, Action::ToLeft)), "a folder on one side, whole");
        assert_eq!(of("sized.txt"), Some((State::Different, Action::Nothing)), "same time, other size");
        let masked = compare(&left, &right, Options::default(), "*.md", &stop, &mut |_| {}).unwrap();
        assert_eq!(masked.iter().map(|item| item.path.clone()).collect::<Vec<_>>(), [PathBuf::from("note.md")]);
        let mirror = Options { asymmetric: true, ..Options::default() };
        let mirrored = compare(&left, &right, mirror, "", &stop, &mut |_| {}).unwrap();
        let only_right = mirrored.iter().find(|item| item.path == Path::new("only-right")).unwrap();
        assert_eq!(only_right.action, Action::DeleteRight);
        // Done: the right side now has what the left had newer or alone, and the other way.
        let queue = crate::ops::Queue::default();
        queue.push_task(synchronize(left.clone(), right.clone(), found));
        while !queue.jobs().iter().all(|job| job.status.finished()) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(fs::read_to_string(b.join("newer.txt")).unwrap(), "new");
        assert_eq!(fs::read_to_string(b.join("sub/only-left.txt")).unwrap(), "l");
        assert_eq!(fs::read_to_string(a.join("only-right/x.txt")).unwrap(), "r");
        assert_eq!(fs::read_to_string(b.join("sized.txt")).unwrap(), "three", "left alone");
        // Changed after the comparison: not gone over.
        touch(&a.join("newer.txt"), "newest", 1);
        let stale = compare(&left, &right, Options::default(), "", &stop, &mut |_| {}).unwrap();
        touch(&b.join("newer.txt"), "edited meanwhile", 0);
        queue.push_task(synchronize(left.clone(), right.clone(), stale));
        while !queue.jobs().iter().all(|job| job.status.finished()) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(fs::read_to_string(b.join("newer.txt")).unwrap(), "edited meanwhile");
        touch(&a.join("newer.txt"), "new", 10);
        touch(&b.join("newer.txt"), "new", 10);
        let again = compare(&left, &right, Options::default(), "", &stop, &mut |_| {}).unwrap();
        assert!(again.iter().all(|item| item.action == Action::Nothing), "{again:?}");
        fs::remove_dir_all(&base).unwrap();
    }
}
