//! Deleting this computer's files: to the trash (Undo puts them back), else
//! for good, one by one, so a task shows how far it got and stops when cancelled.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::activity;
use crate::ops::{Task, undo};

/// What deleting `paths` in `folder` is called in the queue and the activity log.
pub fn title(paths: &[PathBuf], folder: &Path, to_trash: bool) -> String {
    let name = |path: &Path| path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    match (paths, to_trash) {
        ([one], false) => format!("Delete \"{}\"", name(one)),
        ([one], true) => format!("Move \"{}\" to the trash", name(one)),
        (many, false) => format!("Delete {} items in {}", many.len(), folder.display()),
        (many, true) => format!("Move {} items in {} to the trash", many.len(), folder.display()),
    }
}

/// Deletes `paths`, to the trash or for good; `each` hears every path
/// before it goes, and stops the rest with an error. What went to the
/// trash is remembered for Undo, also when it stopped.
pub fn remove(paths: &[PathBuf], to_trash: bool, title: &str, each: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
    let at = std::time::SystemTime::now();
    let mut trashed = Vec::new();
    let removed = paths.iter().try_for_each(|path| match to_trash {
        true => {
            each(path)?;
            undo::to_trash(path)?;
            trashed.push(path.clone());
            Ok(())
        }
        false => remove_tree(path, each),
    });
    activity::note(title.to_owned(), &removed);
    if !trashed.is_empty() {
        undo::remember(title.to_owned(), undo::Step::Trashed { paths: trashed, at, displaced: Vec::new() });
    }
    removed
}

/// A folder's contents first, then the folder; a link goes, never what it points to.
fn remove_tree(path: &Path, each: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            remove_tree(&entry?.path(), each)?;
        }
        each(path)?;
        return fs::remove_dir(path);
    }
    each(path)?;
    match fs::remove_file(path) {
        // Windows keeps a read-only file; inside what is deleted, it goes too.
        #[cfg(windows)]
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied && meta.permissions().readonly() => {
            let mut writable = meta.permissions();
            // Windows only: it clears the read-only flag, nothing more.
            #[allow(clippy::permissions_set_readonly_false)]
            writable.set_readonly(false);
            fs::set_permissions(path, writable)?;
            fs::remove_file(path)
        }
        removed => removed,
    }
}

/// How many entries `path` holds, itself among them; stops when `stop` says so.
fn count(path: &Path, stop: &dyn Fn() -> io::Result<()>) -> io::Result<u64> {
    stop()?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() {
        return Ok(1);
    }
    let mut total = 1;
    for entry in fs::read_dir(path)? {
        total += count(&entry?.path(), stop)?;
    }
    Ok(total)
}

/// Deletes `paths` as a task: each file counted (for good) or each item
/// (to the trash) shown as it goes; `done` hears how it ended.
pub fn task(title: String, paths: Vec<PathBuf>, to_trash: bool, done: impl FnOnce(Option<io::ErrorKind>) + Send + 'static) -> Task {
    Task {
        title: title.clone(),
        run: Box::new(move |report| {
            report.progress.total_files = paths.len() as u64;
            if !to_trash {
                report.progress.scanning = true;
                report.show();
                let mut total = 0;
                for path in &paths {
                    total += count(path, &|| report.check()).unwrap_or(1);
                    report.progress.total_files = total;
                    report.show();
                }
                report.progress.scanning = false;
            }
            let removed = remove(&paths, to_trash, &title, &mut |path| {
                report.check()?;
                report.progress.current = path.to_owned();
                report.progress.files += 1;
                report.show();
                Ok(())
            });
            done(removed.as_ref().err().map(io::Error::kind));
            removed.map(|()| String::new())
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletes_trees_and_counts_them() {
        let root = std::env::temp_dir().join(format!("sc-delete-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for dir in ["a/b/c", "d"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["a/1", "a/b/2", "a/b/c/3", "e"] {
            fs::write(root.join(file), "x").unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("d"), root.join("a/link")).unwrap();
        let paths = vec![root.join("a"), root.join("e")];
        assert_eq!(count(&root.join("a"), &|| Ok(())).unwrap(), if cfg!(unix) { 7 } else { 6 });
        let mut seen = 0;
        remove(&paths, false, "test", &mut |_| {
            seen += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, if cfg!(unix) { 8 } else { 7 });
        assert!(!root.join("a").exists() && !root.join("e").exists());
        assert!(root.join("d").is_dir(), "a link's target stays");
        // Stopped halfway: what is left stays.
        fs::create_dir_all(root.join("f/g")).unwrap();
        let stopped = remove(&[root.join("f")], false, "test", &mut |_| Err(io::ErrorKind::Interrupted.into()));
        assert!(stopped.is_err() && root.join("f/g").is_dir());
        fs::remove_dir_all(&root).unwrap();
    }
}
