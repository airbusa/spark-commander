//! Commands > Undo (Ctrl+Z): the last copies, moves, renames and moves to the
//! trash, undone newest first; Redo (Ctrl+Shift+Z) does the undone again. What
//! a copy or move went over on this computer went to the trash, and comes back
//! from there; on a server it is gone.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::SystemTime;

use crate::ops::{Kind, Operation, Overwrite, Queue, Target, Task};
use crate::remote::{Address, Ask};
use crate::vfs::{self, Fs};

/// Where a step's files are: this computer, or a server kept by its address,
/// so Undo holds no connection open; the one a tab still keeps is used.
#[derive(Clone)]
pub enum Place {
    Here,
    Server { url: String, open: Weak<dyn Fs> },
}

impl Place {
    pub fn of(fs: &Arc<dyn Fs>) -> Self {
        // A server's id is its address.
        match fs.id().as_str() {
            "local" => Place::Here,
            url => Place::Server { url: url.to_owned(), open: Arc::downgrade(fs) },
        }
    }

    /// The file system: the connection a tab still keeps, else a new one,
    /// `ask` answering its login. Blocks.
    fn open(&self, ask: &Ask) -> io::Result<Arc<dyn Fs>> {
        match self {
            Place::Here => Ok(Arc::new(vfs::Local)),
            Place::Server { url, open } => match open.upgrade() {
                Some(fs) => Ok(fs),
                None => {
                    let address = Address::parse(url).ok_or_else(|| io::Error::other(crate::t!("ops.undo.not_a_server", url = url)))?;
                    let (server, _) = crate::remote::connect(&address, ask.clone())?;
                    Ok(server)
                }
            },
        }
    }
}

#[derive(Clone)]
pub enum Step {
    /// New copies: removed again, to the trash on this computer; what they
    /// went over, `replaced`, back from the trash, where it went since `at`.
    Copied { fs: Place, made: Vec<PathBuf>, replaced: Vec<PathBuf>, at: SystemTime },
    /// Moved or renamed, `(was, now)`: back, from `to`'s file system to
    /// `from`'s; then `restore`, which they went over, back from the trash
    /// (since `at`). `replaces`: what doing them again goes over.
    Moved { from: Place, to: Place, moves: Vec<(PathBuf, PathBuf)>, restore: Vec<PathBuf>, replaces: Vec<PathBuf>, at: SystemTime },
    /// Moved to the trash at `at`: restored, once `displaced`, in their place since, went to the trash.
    Trashed { paths: Vec<PathBuf>, at: SystemTime, displaced: Vec<PathBuf> },
}

pub struct Undo {
    /// What was done: "Copy 3 items to /home/me".
    pub title: String,
    pub step: Step,
}

/// How many are kept.
const KEPT: usize = 20;

static DONE: Mutex<Vec<Undo>> = Mutex::new(Vec::new());
/// What was undone, newest last, for Redo; something new done drops it.
static UNDONE: Mutex<Vec<Undo>> = Mutex::new(Vec::new());

fn keep(list: &Mutex<Vec<Undo>>, undo: Undo) {
    let mut list = list.lock().unwrap();
    list.push(undo);
    if list.len() > KEPT {
        list.remove(0);
    }
}

pub fn remember(title: String, step: Step) {
    keep(&DONE, Undo { title, step });
    // Tests run side by side: one's copy would drop another's Redo.
    #[cfg(not(test))]
    UNDONE.lock().unwrap().clear();
}

/// The newest, to undo now.
pub fn take() -> Option<Undo> {
    DONE.lock().unwrap().pop()
}

/// The newest undone, to do again now.
pub fn take_undone() -> Option<Undo> {
    UNDONE.lock().unwrap().pop()
}

/// What undoes `step` in its turn; none for a server's copies, removed for good.
fn reverse(step: &Step) -> Option<Step> {
    let now = SystemTime::now();
    Some(match step {
        // Moved to the trash: back from it.
        Step::Copied { fs: Place::Here, made, replaced, .. } => Step::Trashed { paths: made.clone(), at: now, displaced: replaced.clone() },
        Step::Copied { .. } => return None,
        Step::Moved { from, to, moves, restore, replaces, .. } => Step::Moved {
            from: to.clone(),
            to: from.clone(),
            moves: moves.iter().map(|(was, now)| (now.clone(), was.clone())).collect(),
            restore: replaces.clone(),
            replaces: restore.clone(),
            at: now,
        },
        // Back from the trash: to it again.
        Step::Trashed { paths, displaced, .. } => Step::Copied { fs: Place::Here, made: paths.clone(), replaced: displaced.clone(), at: now },
    })
}

/// Undoes `undo`, or does again what was undone (`redo`): moves go through
/// `queue`, as a copy goes, unrecorded (false: queued, not done yet); the
/// trash here, which blocks. A server no tab keeps open is connected again,
/// `ask` answering its login. Its reverse is kept for the other, Redo or
/// Undo; failed, it is kept to try again.
pub fn run(undo: Undo, queue: &Queue, redo: bool, ask: Ask) -> io::Result<bool> {
    let Undo { title: what, step } = undo;
    let reversed = reverse(&step);
    let kept = (what.clone(), step.clone());
    let queued = matches!(step, Step::Moved { .. });
    let title = match redo {
        true => crate::t!("ops.redo.title", what = what),
        false => crate::t!("ops.undo.title", what = what),
    };
    let done = match step {
        // One gone already (a try before failed after it) is left out.
        Step::Copied { fs: fs @ Place::Server { .. }, made, .. } => {
            let undone = fs.open(&ask).and_then(|fs| made.iter().filter(|path| fs.metadata(path).is_ok()).try_for_each(|path| vfs::remove_all(&*fs, path)));
            crate::activity::note(title, &undone);
            undone
        }
        // Found in the trash before the copies go there, as they have the same names.
        Step::Copied { made, replaced, at, .. } => {
            let undone = in_trash(&replaced, at).and_then(|back| {
                made.iter().filter(|path| std::fs::symlink_metadata(path).is_ok()).try_for_each(|path| to_trash(path))?;
                put_back(back)
            });
            crate::activity::note(title, &undone);
            undone
        }
        Step::Moved { from, to, moves, restore, at, .. } => {
            let (from, to) = (from.open(&ask)?, to.open(&ask)?);
            // One no longer where it went (its move back skipped or cancelled) stays where it is.
            for (was, now) in moves.into_iter().filter(|(_, now)| to.metadata(now).is_ok()) {
                // A merged folder's, moved one by one, went with its last: made again.
                if let Some(parent) = was.parent() {
                    make_dirs(&*from, parent)?;
                }
                // The same name goes into its folder; a renamed one gets its name back.
                let target = match (was.file_name() == now.file_name(), was.parent()) {
                    (true, Some(parent)) => Target::Into(parent.to_owned()),
                    _ => Target::As(was),
                };
                let back = Operation { kind: Kind::Move, from: to.clone(), sources: vec![now], to: from.clone(), target, overwrite: Overwrite::Ask };
                queue.push_reversal(back);
            }
            // After the moves back, the queue doing one at a time.
            if !restore.is_empty() {
                let run = move |_: &mut crate::ops::Report| in_trash(&restore, at).and_then(put_back).map(|()| String::new());
                queue.push_task(Task { title: crate::t!("ops.undo.restore_title", title = title), run: Box::new(run) });
            }
            Ok(())
        }
        // The queue notes its moves back.
        Step::Trashed { paths, at, displaced } => {
            let undone = in_trash(&paths, at).and_then(|back| {
                if back.is_empty() {
                    return Err(io::Error::new(io::ErrorKind::NotFound, crate::t!("ops.undo.not_in_trash")));
                }
                displaced.iter().try_for_each(|path| to_trash(path))?;
                put_back(back)
            });
            crate::activity::note(title, &undone);
            undone
        }
    };
    match &done {
        Ok(()) => {
            if let Some(step) = reversed {
                keep(if redo { &DONE } else { &UNDONE }, Undo { title: what, step });
            }
        }
        Err(_) => keep(if redo { &UNDONE } else { &DONE }, Undo { title: kept.0, step: kept.1 }),
    }
    done.map(|()| !queued)
}

/// `dir`, with the folders above it, made where gone.
fn make_dirs(fs: &dyn Fs, dir: &Path) -> io::Result<()> {
    if dir.as_os_str().is_empty() || fs.metadata(dir).is_ok() {
        return Ok(());
    }
    if let Some(parent) = dir.parent() {
        make_dirs(fs, parent)?;
    }
    fs.mkdir(dir)
}

/// Moves `path` to this computer's trash, as F8 does. On macOS by the
/// system's call, not Finder's, which says where it put it: Undo finds it by that.
#[cfg(all(not(test), not(target_os = "macos")))]
pub fn to_trash(path: &Path) -> io::Result<()> {
    trash::delete(path).map_err(io::Error::other)
}

/// Where this app put things in the trash: `(was, there, when)`.
#[cfg(any(test, target_os = "macos"))]
static TRASHED: Mutex<Vec<(PathBuf, PathBuf, SystemTime)>> = Mutex::new(Vec::new());

/// Tests' trash: a folder of their own, the user's left alone.
#[cfg(test)]
pub fn to_trash(path: &Path) -> io::Result<()> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let bin = std::env::temp_dir().join(format!("sc-test-trash-{}", std::process::id()));
    std::fs::create_dir_all(&bin)?;
    let there = bin.join(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed).to_string());
    std::fs::rename(path, &there)?;
    TRASHED.lock().unwrap().push((path.to_owned(), there, SystemTime::now()));
    Ok(())
}

#[cfg(all(not(test), target_os = "macos"))]
pub fn to_trash(path: &Path) -> io::Result<()> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let path = std::path::absolute(path)?;
    let text = path.to_str().ok_or_else(|| io::Error::other("the name is not UTF-8"))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(text));
    let mut there = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut there))
        .map_err(|err| io::Error::other(err.localizedDescription().to_string()))?;
    if let Some(there) = there.and_then(|url| url.path()) {
        TRASHED.lock().unwrap().push((path, PathBuf::from(there.to_string()), SystemTime::now()));
    }
    Ok(())
}

/// `path`, just put in the trash since `at`, back where it was.
pub fn take_back(path: &Path, at: SystemTime) -> io::Result<()> {
    put_back(in_trash_newest(&[path.to_owned()], at, true)?)
}

/// What `paths` became in the trash, put there since `at`: the first of each
/// since, as a later one of the same name was put there by something else.
fn in_trash(paths: &[PathBuf], at: SystemTime) -> io::Result<Vec<TrashedItem>> {
    in_trash_newest(paths, at, false)
}

#[cfg(all(not(test), any(windows, all(unix, not(target_os = "macos")))))]
type TrashedItem = trash::TrashItem;
#[cfg(any(test, target_os = "macos"))]
type TrashedItem = (PathBuf, PathBuf);
#[cfg(all(not(test), not(any(windows, unix))))]
type TrashedItem = ();

/// What `paths` became in the trash since `at`, the newest or the first of
/// each; none of one no longer there.
#[cfg(all(not(test), any(windows, all(unix, not(target_os = "macos")))))]
fn in_trash_newest(paths: &[PathBuf], at: SystemTime, newest: bool) -> io::Result<Vec<trash::TrashItem>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let since = at.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64) - 2;
    let mut items: Vec<trash::TrashItem> = trash::os_limited::list().map_err(io::Error::other)?;
    items.retain(|item| item.time_deleted >= since && paths.contains(&item.original_path()));
    items.sort_by_key(|item| if newest { -item.time_deleted } else { item.time_deleted });
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert(item.original_path()));
    Ok(items)
}

#[cfg(all(not(test), any(windows, all(unix, not(target_os = "macos")))))]
fn put_back(items: Vec<trash::TrashItem>) -> io::Result<()> {
    match items.is_empty() {
        true => Ok(()),
        false => trash::os_limited::restore_all(items).map_err(io::Error::other),
    }
}

#[cfg(any(test, target_os = "macos"))]
fn in_trash_newest(paths: &[PathBuf], at: SystemTime, newest: bool) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    let since = at - std::time::Duration::from_secs(2);
    let trashed = TRASHED.lock().unwrap();
    let mut found: Vec<(PathBuf, PathBuf)> = Vec::new();
    let order: Box<dyn Iterator<Item = &(PathBuf, PathBuf, SystemTime)>> = match newest {
        true => Box::new(trashed.iter().rev()),
        false => Box::new(trashed.iter()),
    };
    for (was, there, when) in order {
        if *when >= since && paths.contains(was) && there.exists() && !found.iter().any(|(seen, _)| seen == was) {
            found.push((was.clone(), there.clone()));
        }
    }
    Ok(found)
}

/// Each back where it was; one there since stays, and that one is not put back.
#[cfg(any(test, target_os = "macos"))]
fn put_back(items: Vec<(PathBuf, PathBuf)>) -> io::Result<()> {
    for (was, there) in items {
        if std::fs::symlink_metadata(&was).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{}: something of this name is there now", was.display())));
        }
        if let Some(parent) = was.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&there, &was)?;
        TRASHED.lock().unwrap().retain(|(_, kept, _)| *kept != there);
    }
    Ok(())
}

#[cfg(all(not(test), not(any(windows, unix))))]
fn in_trash_newest(_: &[PathBuf], _: SystemTime, _: bool) -> io::Result<Vec<()>> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "the trash here cannot be undone"))
}

#[cfg(all(not(test), not(any(windows, unix))))]
fn put_back(_: Vec<()>) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This computer's files as a server's would be: another id.
    struct Server;

    impl Fs for Server {
        fn id(&self) -> String {
            "sftp://me@server.invalid".into()
        }
        fn metadata(&self, path: &Path) -> io::Result<vfs::Meta> {
            Local.metadata(path)
        }
        fn list(&self, dir: &Path) -> io::Result<Vec<std::ffi::OsString>> {
            Local.list(dir)
        }
        fn open(&self, path: &Path) -> io::Result<Box<dyn io::Read + Send>> {
            Local.open(path)
        }
        fn create(&self, path: &Path) -> io::Result<Box<dyn io::Write + Send>> {
            Local.create(path)
        }
        fn set_meta(&self, path: &Path, meta: &vfs::Meta) -> io::Result<()> {
            Local.set_meta(path, meta)
        }
        fn mkdir(&self, path: &Path) -> io::Result<()> {
            Local.mkdir(path)
        }
        fn remove(&self, path: &Path, is_dir: bool) -> io::Result<()> {
            Local.remove(path, is_dir)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            Local.rename(from, to)
        }
    }

    /// A step keeps no server's connection open; it uses the one a tab keeps.
    #[test]
    fn keeps_servers_by_address() {
        let server: Arc<dyn Fs> = Arc::new(Server);
        let place = Place::of(&server);
        assert_eq!(Arc::strong_count(&server), 1, "not held");
        assert!(Arc::ptr_eq(&place.open(&no_ask()).unwrap(), &server), "the open one");
        assert!(matches!(Place::of(&(Arc::new(Local) as Arc<dyn Fs>)), Place::Here));
        match &place {
            Place::Server { url, .. } => assert_eq!(url, "sftp://me@server.invalid"),
            Place::Here => panic!("a server's"),
        }
    }

    /// Tests' servers ask nothing.
    fn no_ask() -> Ask {
        Arc::new(|_| crate::remote::Reply::Cancel)
    }
    use crate::vfs::Local;
    use std::time::Duration;

    /// The undo list is the whole process's, other tests' too: this one's by its title.
    fn taken(title: &str) -> Option<Undo> {
        taken_from(&DONE, title)
    }

    fn taken_from(list: &Mutex<Vec<Undo>>, title: &str) -> Option<Undo> {
        let mut list = list.lock().unwrap();
        let at = list.iter().position(|undo| undo.title == title)?;
        Some(list.remove(at))
    }

    fn finish(queue: &Queue) {
        while !queue.jobs().iter().all(|job| job.status.finished()) {
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn moves_back_what_was_moved_and_renamed() {
        let dir = std::env::temp_dir().join(format!("sc-undo-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("into")).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        let queue = Queue::default();
        let local = || -> Arc<dyn Fs> { Arc::new(Local) };
        let moved = Operation { kind: Kind::Move, from: local(), sources: vec![dir.join("a.txt")], to: local(), target: Target::Into(dir.join("into")), overwrite: Overwrite::Ask };
        let title = moved.title();
        queue.push(moved);
        finish(&queue);
        assert!(dir.join("into/a.txt").exists());
        let renamed = Operation { kind: Kind::Move, from: local(), sources: vec![dir.join("b.txt")], to: local(), target: Target::As(dir.join("c.txt")), overwrite: Overwrite::Ask };
        let renamed_title = renamed.title();
        queue.push(renamed);
        finish(&queue);
        for title in [title, renamed_title.clone()] {
            run(taken(&title).expect("remembered"), &queue, false, no_ask()).unwrap();
        }
        finish(&queue);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "a", "back in its folder");
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "b", "its name back");
        assert!(!dir.join("into/a.txt").exists() && !dir.join("c.txt").exists());
        // The move back is not remembered in its turn.
        assert!(DONE.lock().unwrap().iter().all(|undo| !undo.title.contains(&*dir.to_string_lossy())));
        // Redo renames it again, and Undo can undo that.
        run(taken_from(&UNDONE, &renamed_title).expect("undone"), &queue, true, no_ask()).unwrap();
        finish(&queue);
        assert!(dir.join("c.txt").exists() && !dir.join("b.txt").exists(), "renamed again");
        assert!(taken(&renamed_title).is_some(), "to undo again");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_try_again_leaves_out_what_is_gone() {
        let dir = std::env::temp_dir().join(format!("sc-undo-again-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b"), "b").unwrap();
        let step = Step::Copied { fs: Place::Here, made: vec![dir.join("a"), dir.join("b")], replaced: Vec::new(), at: SystemTime::now() };
        assert!(run(Undo { title: "Copy".into(), step }, &Queue::default(), false, no_ask()).unwrap());
        assert!(!dir.join("b").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn takes_back_the_version_it_put_in_the_trash() {
        let dir = std::env::temp_dir().join(format!("sc-undo-twice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.txt");
        let at = SystemTime::now();
        std::fs::write(&file, "first").unwrap();
        to_trash(&file).unwrap();
        // Later, another file of that name goes to the trash too.
        std::fs::write(&file, "second").unwrap();
        to_trash(&file).unwrap();
        put_back(in_trash(std::slice::from_ref(&file), at).unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "first");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn takes_back_what_went_over_files() {
        use std::fs::{create_dir_all, read_to_string, write};
        let dir = std::env::temp_dir().join(format!("sc-undo-over-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (src, dst) = (dir.join("src"), dir.join("dst"));
        for folder in [src.join("sub"), dst.join("sub"), src.join("merged"), dst.join("merged")] {
            create_dir_all(folder).unwrap();
        }
        for (path, text) in [("src/a.txt", "new a"), ("src/sub/b.txt", "new b"), ("src/sub/c.txt", "new c"), ("dst/a.txt", "old a"), ("dst/sub/b.txt", "old b"), ("dst/sub/keep.txt", "keep")] {
            write(dir.join(path), text).unwrap();
        }
        let read = |path: &str| read_to_string(dir.join(path)).ok();
        let queue = Queue::default();
        let local = || -> Arc<dyn Fs> { Arc::new(Local) };
        let copy = Operation { kind: Kind::Copy, from: local(), sources: vec![src.join("a.txt"), src.join("sub")], to: local(), target: Target::Into(dst.clone()), overwrite: Overwrite::All };
        let title = copy.title();
        queue.push(copy);
        finish(&queue);
        let new = |read: &dyn Fn(&str) -> Option<String>| {
            assert_eq!(read("dst/a.txt").as_deref(), Some("new a"));
            assert_eq!(read("dst/sub/b.txt").as_deref(), Some("new b"));
            assert_eq!(read("dst/sub/c.txt").as_deref(), Some("new c"));
        };
        let old = |read: &dyn Fn(&str) -> Option<String>| {
            assert_eq!(read("dst/a.txt").as_deref(), Some("old a"), "back from the trash");
            assert_eq!(read("dst/sub/b.txt").as_deref(), Some("old b"), "in a merged folder too");
            assert_eq!(read("dst/sub/c.txt"), None, "a new one in a merged folder goes");
        };
        new(&read);
        run(taken(&title).expect("remembered"), &queue, false, no_ask()).unwrap();
        old(&read);
        run(taken_from(&UNDONE, &title).expect("undone"), &queue, true, no_ask()).unwrap();
        new(&read);
        run(taken(&title).expect("redone"), &queue, false, no_ask()).unwrap();
        old(&read);
        assert_eq!(read("dst/sub/keep.txt").as_deref(), Some("keep"), "what was there alone stays");

        // A move over a file, and into a folder there already, which then goes from the source.
        write(dir.join("src/merged/m.txt"), "moved").unwrap();
        write(dir.join("dst/merged/m.txt"), "old m").unwrap();
        write(dir.join("src/merged/n.txt"), "n").unwrap();
        let moved = Operation { kind: Kind::Move, from: local(), sources: vec![src.join("merged")], to: local(), target: Target::Into(dst.clone()), overwrite: Overwrite::All };
        let title = moved.title();
        queue.push(moved);
        finish(&queue);
        assert!(!src.join("merged").exists());
        assert_eq!(read("dst/merged/m.txt").as_deref(), Some("moved"));
        run(taken(&title).expect("remembered"), &queue, false, no_ask()).unwrap();
        finish(&queue);
        assert_eq!(read("src/merged/m.txt").as_deref(), Some("moved"), "moved back");
        assert_eq!(read("src/merged/n.txt").as_deref(), Some("n"));
        assert_eq!(read("dst/merged/m.txt").as_deref(), Some("old m"), "then back from the trash");
        assert_eq!(read("dst/merged/n.txt"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
