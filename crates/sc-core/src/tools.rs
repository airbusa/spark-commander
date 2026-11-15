//! The Tools menu: the user's own command lines, on keys too, with Total
//! Commander's placeholders for the panels' folders and files.

use std::ffi::OsString;
use std::path::{MAIN_SEPARATOR, Path};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tool {
    pub name: String,
    pub command: String,
    /// GPUI's way of writing it (`ctrl-shift-t`); empty for none.
    pub key: String,
    /// In the built-in terminal, its output seen; else on its own.
    pub terminal: bool,
}

/// What the placeholders stand for.
pub struct Places<'a> {
    pub folder: &'a Path,
    /// The other panel's folder; none on a server or in an archive.
    pub other: Option<&'a Path>,
    /// The file or folder under the cursor; none on `..`.
    pub name: Option<OsString>,
    pub selected: Vec<OsString>,
}

/// Why `command` cannot run with `places`: a placeholder with nothing to stand for.
pub fn missing(command: &str, places: &Places) -> Option<&'static str> {
    if command.contains("%T") && places.other.is_none() {
        return Some("The other panel shows a server or an archive: %T needs a folder on this computer.");
    }
    let named = ["%N", "%O", "%E"].iter().any(|placeholder| command.contains(placeholder));
    (named && places.name.is_none()).then_some("The cursor is on no file: %N, %O and %E need one.")
}

/// `command` filled in: %P this panel's folder and %T the other's, each ending
/// in a separator, so "%P%N" is a path; %N the name under the cursor, %O
/// it without its extension, %E its extension; %S the selected names (else
/// the cursor's), each quoted; %% a %. Only %S is quoted: quote the rest.
pub fn expand(command: &str, places: &Places) -> String {
    let folder = |path: &Path| {
        let text = path.display().to_string();
        if text.ends_with(MAIN_SEPARATOR) { text } else { format!("{text}{MAIN_SEPARATOR}") }
    };
    let name = places.name.as_ref().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, extension) = match name.rfind('.').filter(|&dot| dot > 0) {
        Some(dot) => (name[..dot].to_owned(), name[dot + 1..].to_owned()),
        None => (name.clone(), String::new()),
    };
    let selected: Vec<String> = match places.selected.is_empty() {
        true => places.name.iter().map(|name| quote(&name.to_string_lossy())).collect(),
        false => places.selected.iter().map(|name| quote(&name.to_string_lossy())).collect(),
    };
    let mut out = String::new();
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        let filled = match (c, chars.peek()) {
            ('%', Some('P')) => folder(places.folder),
            ('%', Some('T')) => places.other.map(folder).unwrap_or_default(),
            ('%', Some('N')) => name.clone(),
            ('%', Some('O')) => stem.clone(),
            ('%', Some('E')) => extension.clone(),
            ('%', Some('S')) => selected.join(" "),
            ('%', Some('%')) => "%".to_owned(),
            _ => {
                out.push(c);
                continue;
            }
        };
        chars.next();
        out.push_str(&filled);
    }
    out
}

/// One argument for the shell, quoted only when it needs it.
fn quote(text: &str) -> String {
    let plain = !text.is_empty() && text.chars().all(|c| c.is_alphanumeric() || "._-+/=:,@".contains(c));
    match (plain, cfg!(windows)) {
        (true, _) => text.to_owned(),
        (false, true) => format!("\"{text}\""),
        (false, false) => format!("'{}'", text.replace('\'', r"'\''")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn fills_in_total_commanders_placeholders() {
        let places = Places {
            folder: Path::new("/home/me/docs"),
            other: Some(Path::new("/backup/")),
            name: Some("report final.pdf".into()),
            selected: Vec::new(),
        };
        assert_eq!(expand("gimp \"%P%N\"", &places), "gimp \"/home/me/docs/report final.pdf\"");
        assert_eq!(expand("cp %S %T; echo %O.%E 100%%", &places), "cp 'report final.pdf' /backup/; echo report final.pdf 100%");
        let marked = Places { selected: vec!["a.txt".into(), "it's.txt".into()], ..places };
        assert_eq!(expand("tar cf x.tar %S", &marked), r"tar cf x.tar a.txt 'it'\''s.txt'");
        assert_eq!(missing("cat %N", &Places { name: None, ..marked }), Some("The cursor is on no file: %N, %O and %E need one."));
        assert_eq!(expand("%X stays", &Places { folder: Path::new("/"), other: None, name: None, selected: Vec::new() }), "%X stays");
    }
}
