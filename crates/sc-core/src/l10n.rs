//! The app's words in the user's language. `l10n/<tag>.json` hold them as
//! remoxio's do: each key's ICU message (`{name}`, `{count, plural, one {…}
//! other {…}}`, `{kind, select, …}`); English, `en.json` with notes for the
//! translators, is the source and every language's fallback. The language is
//! the one chosen in Settings, else the system's.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use icu_plurals::{PluralCategory, PluralRules};
use serde::Deserialize;

include!(concat!(env!("OUT_DIR"), "/l10n.rs"));

/// `t!("key")`, or `t!("key", name = value, …)`: the key's message in the
/// app's language, its `{name}`s filled in.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::l10n::tr($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::l10n::tr_args($key, &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+])
    };
}

#[derive(Deserialize)]
struct Entry {
    message: String,
}

type Catalog = HashMap<String, String>;

struct Current {
    tag: &'static str,
    own: Arc<Catalog>,
    english: Arc<Catalog>,
    plurals: Option<PluralRules>,
}

static CURRENT: RwLock<Option<Arc<Current>>> = RwLock::new(None);

/// A language's messages, read from its file once.
fn catalog(tag: &'static str) -> Arc<Catalog> {
    static READ: OnceLock<Mutex<HashMap<&'static str, Arc<Catalog>>>> = OnceLock::new();
    let mut read = READ.get_or_init(Default::default).lock().unwrap();
    read.entry(tag)
        .or_insert_with(|| {
            let text = FILES.iter().find(|(file, _)| *file == tag).map_or("{}", |(_, text)| text);
            let entries: HashMap<String, Entry> = serde_json::from_str(text).unwrap_or_default();
            Arc::new(entries.into_iter().map(|(key, entry)| (key, entry.message)).collect())
        })
        .clone()
}

/// The tag of ours a language tag (`hu-HU`, `zh_TW`, `pt`) comes to, if any.
fn supported(tag: &str) -> Option<&'static str> {
    let ours: Vec<&'static str> = FILES.iter().map(|(tag, _)| *tag).collect();
    pick(tag, &ours)
}

/// Which of `ours` a language tag comes to.
fn pick<'a>(tag: &str, ours: &[&'a str]) -> Option<&'a str> {
    let tag = tag.split(['.', '@']).next().unwrap_or_default().replace('_', "-");
    let ours = |wanted: &str| ours.iter().copied().find(|tag| tag.eq_ignore_ascii_case(wanted));
    let lower = tag.to_ascii_lowercase();
    let language = lower.split('-').next().unwrap_or_default();
    ours(&tag).or_else(|| match language {
        "zh" if ["hant", "tw", "hk", "mo"].iter().any(|traditional| lower.split('-').any(|part| part == *traditional)) => ours("zh-Hant"),
        "zh" => ours("zh-Hans"),
        "pt" if lower.split('-').any(|part| part == "br") => ours("pt-BR"),
        "pt" => ours("pt-PT"),
        "no" | "nn" => ours("nb"),
        language => ours(language),
    })
}

/// The system's language, where the app speaks it.
pub fn system_language() -> Option<String> {
    sys_locale::get_locale().and_then(|tag| supported(&tag)).map(str::to_owned)
}

/// Chooses the language: `choice` (a tag), else the system's, else English.
pub fn set_language(choice: Option<&str>) {
    let system = || sys_locale::get_locale().and_then(|tag| supported(&tag));
    let tag = choice.and_then(supported).or_else(system).unwrap_or("en");
    if CURRENT.read().unwrap().as_ref().is_some_and(|current| current.tag == tag) {
        return;
    }
    let locale: Option<icu_locale_core::Locale> = tag.parse().ok();
    let plurals = locale.and_then(|locale| PluralRules::try_new_cardinal(locale.into()).ok());
    let current = Current { tag, own: catalog(tag), english: catalog("en"), plurals };
    *CURRENT.write().unwrap() = Some(Arc::new(current));
}

fn current() -> Arc<Current> {
    if let Some(current) = CURRENT.read().unwrap().as_ref() {
        return current.clone();
    }
    set_language(None);
    CURRENT.read().unwrap().clone().expect("set")
}

/// The language in use, by its tag.
pub fn language() -> &'static str {
    current().tag
}

/// A language the app speaks.
#[derive(Clone, Debug, PartialEq)]
pub struct Language {
    pub tag: String,
    /// In English: German.
    pub name: String,
    /// In itself: Deutsch.
    pub native: String,
}

/// The languages with a file, in `locales.json`'s order.
pub fn languages() -> Vec<Language> {
    #[derive(Deserialize)]
    struct Locales {
        locales: Vec<Locale>,
    }
    #[derive(Deserialize)]
    struct Locale {
        tag: String,
        name: String,
        native: String,
    }
    let locales: Locales = serde_json::from_str(LOCALES).expect("l10n/locales.json");
    let filed = |tag: &str| FILES.iter().any(|(file, _)| *file == tag);
    locales.locales.into_iter().filter(|locale| filed(&locale.tag)).map(|locale| Language { tag: locale.tag, name: locale.name, native: locale.native }).collect()
}

/// The key's message in the app's language (English where it has none); a
/// key English lacks shows itself, which the tests catch.
pub fn tr(key: &str) -> String {
    tr_args(key, &[])
}

/// As `tr`, its `{name}`s filled in from `args`.
pub fn tr_args(key: &str, args: &[(&str, &dyn Display)]) -> String {
    let current = current();
    let message = current.own.get(key).or_else(|| current.english.get(key));
    match message {
        Some(message) => format(message, args, current.plurals.as_ref()),
        None => key.to_owned(),
    }
}

/// An ICU message with `args`: `{name}`, plurals (`#` the number; `=2`
/// before the category) and selects; `''` is an apostrophe, and one before
/// `{`, `}` or `#` quotes up to the next.
fn format(message: &str, args: &[(&str, &dyn Display)], plurals: Option<&PluralRules>) -> String {
    let chars: Vec<char> = message.chars().collect();
    let mut at = 0;
    let mut out = String::new();
    text(&chars, &mut at, args, plurals, None, &mut out);
    out
}

/// Text up to a closing `}` (inside a plural or select) or the end.
fn text(chars: &[char], at: &mut usize, args: &[(&str, &dyn Display)], plurals: Option<&PluralRules>, number: Option<&str>, out: &mut String) {
    while let Some(&c) = chars.get(*at) {
        match c {
            '}' => return,
            '\'' if chars.get(*at + 1) == Some(&'\'') => {
                out.push('\'');
                *at += 2;
            }
            '\'' if matches!(chars.get(*at + 1), Some('{' | '}' | '#')) => {
                *at += 1;
                while let Some(&quoted) = chars.get(*at) {
                    *at += 1;
                    if quoted == '\'' {
                        break;
                    }
                    out.push(quoted);
                }
            }
            '#' if number.is_some() => {
                out.push_str(number.unwrap_or_default());
                *at += 1;
            }
            '{' => {
                *at += 1;
                argument(chars, at, args, plurals, out);
            }
            c => {
                out.push(c);
                *at += 1;
            }
        }
    }
}

/// After `{`: a value, or a plural or select of it, up to its `}`.
fn argument(chars: &[char], at: &mut usize, args: &[(&str, &dyn Display)], plurals: Option<&PluralRules>, out: &mut String) {
    let word = |at: &mut usize| {
        while chars.get(*at).is_some_and(|c| c.is_whitespace()) {
            *at += 1;
        }
        let start = *at;
        while chars.get(*at).is_some_and(|c| !matches!(c, ',' | '{' | '}') && !c.is_whitespace()) {
            *at += 1;
        }
        let found: String = chars[start..*at].iter().collect();
        while chars.get(*at).is_some_and(|c| c.is_whitespace()) {
            *at += 1;
        }
        found
    };
    let name = word(at);
    let value = args.iter().find(|(arg, _)| *arg == name).map(|(_, value)| value.to_string());
    if chars.get(*at) == Some(&'}') {
        *at += 1;
        out.push_str(&value.unwrap_or_else(|| format!("{{{name}}}")));
        return;
    }
    // `, plural,` or `, select,`, then `choice {message}` pairs.
    *at += 1;
    let kind = word(at);
    *at += 1;
    let value = value.unwrap_or_default();
    let number: Option<i64> = value.trim().parse().ok();
    let category = match (kind.as_str(), number, plurals) {
        ("plural", Some(number), Some(rules)) => match rules.category_for(number) {
            PluralCategory::Zero => "zero",
            PluralCategory::One => "one",
            PluralCategory::Two => "two",
            PluralCategory::Few => "few",
            PluralCategory::Many => "many",
            PluralCategory::Other => "other",
        },
        ("plural", Some(1), None) => "one",
        _ => "other",
    };
    let exact = format!("={value}");
    // The choice's message: an exact one first, then the category, then `other`.
    let mut options: Vec<(String, usize)> = Vec::new();
    loop {
        let choice = word(at);
        match chars.get(*at) {
            Some('{') => {
                *at += 1;
                options.push((choice, *at));
                skip(chars, at);
                *at += 1;
            }
            _ => break,
        }
    }
    *at += 1;
    let wanted = match kind.as_str() {
        "plural" => [exact.as_str(), category, "other"],
        _ => [value.as_str(), "other", "other"],
    };
    let chosen = wanted.iter().find_map(|wanted| options.iter().find(|(choice, _)| choice == wanted));
    if let Some((_, start)) = chosen {
        let mut inner = *start;
        let shown = (kind == "plural").then_some(value.as_str());
        text(chars, &mut inner, args, plurals, shown, out);
    }
}

/// Past a message to its closing `}`, nested ones and quotes too.
fn skip(chars: &[char], at: &mut usize) {
    let mut depth = 0;
    while let Some(&c) = chars.get(*at) {
        match c {
            '\'' if chars.get(*at + 1) == Some(&'\'') => *at += 1,
            '\'' if matches!(chars.get(*at + 1), Some('{' | '}' | '#')) => {
                *at += 1;
                while chars.get(*at + 1).is_some_and(|&quoted| quoted != '\'') {
                    *at += 1;
                }
                *at += 1;
            }
            '{' => depth += 1,
            '}' if depth == 0 => return,
            '}' => depth -= 1,
            _ => {}
        }
        *at += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(tag: &str) -> PluralRules {
        PluralRules::try_new_cardinal(tag.parse::<icu_locale_core::Locale>().unwrap().into()).unwrap()
    }

    #[test]
    fn formats_icu_messages() {
        let en = rules("en");
        let one = |message: &str, args: &[(&str, &dyn Display)]| format(message, args, Some(&en));
        assert_eq!(one("Copy {name} to {folder}", &[("name", &"a.txt"), ("folder", &"/tmp")]), "Copy a.txt to /tmp");
        let files = "{count, plural, =0 {No files} one {# file} other {# files}}";
        assert_eq!(one(files, &[("count", &0)]), "No files");
        assert_eq!(one(files, &[("count", &1)]), "1 file");
        assert_eq!(one(files, &[("count", &7)]), "7 files");
        assert_eq!(one("{kind, select, dir {Folder} other {File}} {name}", &[("kind", &"dir"), ("name", &"x")]), "Folder x");
        assert_eq!(one("It's '{'not'}' {a}", &[("a", &1)]), "It's {not} 1");
        assert_eq!(one("l'élément {name}", &[("name", &"x")]), "l'élément x");
        assert_eq!(one("{missing} stays", &[]), "{missing} stays");
        // Polish: few and many.
        let pl = rules("pl");
        let pliki = "{count, plural, one {# plik} few {# pliki} many {# plików} other {# pliku}}";
        let polish = |count: i64| format(pliki, &[("count", &count)], Some(&pl));
        assert_eq!((polish(1), polish(3), polish(5), polish(22)), ("1 plik".into(), "3 pliki".into(), "5 plików".into(), "22 pliki".into()));
    }

    #[test]
    fn matches_system_tags() {
        #[derive(Deserialize)]
        struct Locales {
            locales: Vec<Tag>,
        }
        #[derive(Deserialize)]
        struct Tag {
            tag: String,
        }
        let locales: Locales = serde_json::from_str(LOCALES).unwrap();
        let tags: Vec<&str> = locales.locales.iter().map(|locale| locale.tag.as_str()).collect();
        let pick = |tag: &str| pick(tag, &tags);
        assert_eq!(pick("hu_HU.UTF-8"), Some("hu"));
        assert_eq!(pick("de-AT"), Some("de"));
        assert_eq!(pick("zh_TW"), Some("zh-Hant"));
        assert_eq!(pick("zh-Hant-HK"), Some("zh-Hant"));
        assert_eq!(pick("zh-CN"), Some("zh-Hans"));
        assert_eq!(pick("pt_BR"), Some("pt-BR"));
        assert_eq!(pick("pt"), Some("pt-PT"));
        assert_eq!(pick("no-NO"), Some("nb"));
        assert_eq!((pick("C"), pick("POSIX")), (None, None));
    }

    /// The names of a message's arguments, those inside plurals and selects too.
    fn arguments(message: &str) -> Vec<String> {
        fn walk(chars: &[char], at: &mut usize, names: &mut Vec<String>) {
            while let Some(&c) = chars.get(*at) {
                *at += 1;
                match c {
                    '}' => return,
                    '\'' if chars.get(*at) == Some(&'\'') => *at += 1,
                    '\'' if matches!(chars.get(*at), Some('{' | '}' | '#')) => {
                        while chars.get(*at).is_some_and(|&quoted| quoted != '\'') {
                            *at += 1;
                        }
                        *at += 1;
                    }
                    '{' => {
                        let start = *at;
                        while chars.get(*at).is_some_and(|c| !matches!(c, ',' | '}')) {
                            *at += 1;
                        }
                        let name: String = chars[start..*at].iter().collect::<String>().trim().to_owned();
                        if !names.contains(&name) {
                            names.push(name);
                        }
                        if chars.get(*at) == Some(&'}') {
                            *at += 1;
                            continue;
                        }
                        // Past the kind to the choices, each walked.
                        *at += 1;
                        while chars.get(*at).is_some_and(|&c| c != ',') {
                            *at += 1;
                        }
                        *at += 1;
                        loop {
                            while chars.get(*at).is_some_and(|&c| c != '{' && c != '}') {
                                *at += 1;
                            }
                            match chars.get(*at) {
                                Some('{') => {
                                    *at += 1;
                                    walk(chars, at, names);
                                }
                                _ => {
                                    *at += 1;
                                    break;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let chars: Vec<char> = message.chars().collect();
        let mut names = Vec::new();
        walk(&chars, &mut 0, &mut names);
        names.sort();
        names
    }

    #[test]
    fn finds_arguments() {
        assert_eq!(arguments("{kind, select, dir {Folder {name}} other {File}} in {where}"), ["kind", "name", "where"]);
        assert_eq!(arguments("{count, plural, one {# file} other {# files}}"), ["count"]);
        assert_eq!(arguments("It''s '{'not'}'"), Vec::<String>::new());
    }

    /// Every `t!("…")` of the code has its English.
    #[test]
    fn code_keys_are_in_english() {
        fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    sources(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
        }
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = Vec::new();
        for name in ["sc-core", "sc-gui", "sc-tui"] {
            sources(&crates.join(name).join("src"), &mut files);
        }
        let mut english: std::collections::HashSet<String> = catalog("en").keys().cloned().collect();
        // Parts not yet merged into en.json (l10n/parts/*.json) count too.
        let parts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../l10n/parts");
        for part in std::fs::read_dir(parts).into_iter().flatten().flatten() {
            let entries: HashMap<String, Entry> = serde_json::from_str(&std::fs::read_to_string(part.path()).unwrap()).unwrap();
            english.extend(entries.into_keys());
        }
        let pattern = regex::Regex::new(r#"\bt!\(\s*"([a-z0-9_]+\.[a-z0-9_.]+)""#).unwrap();
        let mut missing = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            for found in pattern.captures_iter(&text) {
                if !english.contains(&found[1]) {
                    missing.push(format!("{}: {}", file.display(), &found[1]));
                }
            }
        }
        assert!(missing.is_empty(), "not in l10n/en.json:\n{}", missing.join("\n"));
    }

    /// Every language's keys are English's, with its arguments.
    #[test]
    fn translations_match_english() {
        let english = catalog("en");
        for (tag, _) in FILES {
            let own = catalog(tag);
            for (key, message) in own.iter() {
                let source = english.get(key).unwrap_or_else(|| panic!("{tag}: {key} is not in en.json"));
                let (wanted, have) = (arguments(source), arguments(message));
                assert_eq!(wanted, have, "{tag}: {key}: {message}");
            }
        }
    }
}
