//! Commands > Undo (Ctrl+Z): the last copies, moves, renames and moves to the
//! trash, undone newest first; Redo (Ctrl+Shift+Z) does the undone again. What
//! overwrote or joined something there already is not kept: what was there is gone.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::ops::{Kind, Operation, Overwrite, Queue, Target};
use crate::vfs::{self, Fs};

#[derive(Clone)]
pub enum Step {
    /// New copies: removed again, to the trash on this computer.
    Copied { fs: Arc<dyn Fs>, made: Vec<PathBuf> },
    /// Moved or renamed, `(was, now)`: back, from `to`'s file system to `from`'s.
    Moved { from: Arc<dyn Fs>, to: Arc<dyn Fs>, moves: Vec<(PathBuf, PathBuf)> },
    /// Moved to the trash at `at`: restored.
    Trashed { paths: Vec<PathBuf>, at: SystemTime },
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
    Some(match step {
        // Moved to the trash: back from it.
        Step::Copied { fs, made } if fs.id() == "local" => Step::Trashed { paths: made.clone(), at: SystemTime::now() },
        Step::Copied { .. } => return None,
        Step::Moved { from, to, moves } => Step::Moved {
            from: to.clone(),
            to: from.clone(),
            moves: moves.iter().map(|(was, now)| (now.clone(), was.clone())).collect(),
        },
        // Back from the trash: to it again.
        Step::Trashed { paths, .. } => Step::Copied { fs: Arc::new(vfs::Local), made: paths.clone() },
    })
}

/// Undoes `undo`, or does again what was undone (`redo`): moves go through
/// `queue`, as a copy goes, unrecorded; the trash here, which blocks. Its
/// reverse is kept for the other, Redo or Undo.
pub fn run(undo: Undo, queue: &Queue, redo: bool) -> io::Result<()> {
    let Undo { title: what, step } = undo;
    let reversed = reverse(&step);
    let title = format!("{}: {what}", if redo { "Redo" } else { "Undo" });
    let done = match step {
        Step::Copied { fs, made } => {
            let undone = made.iter().try_for_each(|path| match fs.id() == "local" {
                true => trash::delete(path).map_err(io::Error::other),
                false => vfs::remove_all(&*fs, path),
            });
            crate::activity::note(title, &undone);
            undone
        }
        Step::Moved { from, to, moves } => {
            for (was, now) in moves {
                // The same name goes into its folder; a renamed one gets its name back.
                let target = match (was.file_name() == now.file_name(), was.parent()) {
                    (true, Some(parent)) => Target::Into(parent.to_owned()),
                    _ => Target::As(was),
                };
                let back = Operation { kind: Kind::Move, from: to.clone(), sources: vec![now], to: from.clone(), target, overwrite: Overwrite::Ask };
                queue.push_reversal(back);
            }
            Ok(())
        }
        // The queue notes its moves back.
        Step::Trashed { paths, at } => {
            let undone = restore(&paths, at);
            crate::activity::note(title, &undone);
            undone
        }
    };
    if done.is_ok()
        && let Some(step) = reversed
    {
        keep(if redo { &DONE } else { &UNDONE }, Undo { title: what, step });
    }
    done
}

/// Each of `paths` back from the trash: the newest of its name, put there since `at`.
#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
fn restore(paths: &[PathBuf], at: SystemTime) -> io::Result<()> {
    let since = at.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64) - 2;
    let mut items: Vec<trash::TrashItem> = trash::os_limited::list().map_err(io::Error::other)?;
    items.retain(|item| item.time_deleted >= since && paths.contains(&item.original_path()));
    items.sort_by_key(|item| std::cmp::Reverse(item.time_deleted));
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert(item.original_path()));
    if items.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "they are no longer in the trash"));
    }
    trash::os_limited::restore_all(items).map_err(io::Error::other)
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
fn restore(_: &[PathBuf], _: SystemTime) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "the trash here cannot be undone: Finder's Put Back can"))
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let dir = std::env::temp_dir().join(format!("pc-undo-test-{}", std::process::id()));
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
            run(taken(&title).expect("remembered"), &queue, false).unwrap();
        }
        finish(&queue);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "a", "back in its folder");
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "b", "its name back");
        assert!(!dir.join("into/a.txt").exists() && !dir.join("c.txt").exists());
        // The move back is not remembered in its turn.
        assert!(DONE.lock().unwrap().iter().all(|undo| !undo.title.contains(&*dir.to_string_lossy())));
        // Redo renames it again, and Undo can undo that.
        run(taken_from(&UNDONE, &renamed_title).expect("undone"), &queue, true).unwrap();
        finish(&queue);
        assert!(dir.join("c.txt").exists() && !dir.join("b.txt").exists(), "renamed again");
        assert!(taken(&renamed_title).is_some(), "to undo again");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
