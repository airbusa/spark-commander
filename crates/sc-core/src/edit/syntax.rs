//! Colours by rules, as mcedit's: a language's keywords, strings, comments
//! and numbers, a line at a time, what a line leaves open (a block comment,
//! a long string, a tag, a code block) carried into the next. Our own rules,
//! small beside tree-sitter's grammars, for the terminal's editor and viewer.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keyword,
    String,
    Comment,
    Number,
    /// Tags, headings, sections.
    Markup,
    /// A diff's lines.
    Added,
    Removed,
}

/// What a line leaves open for the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Open {
    #[default]
    Nothing,
    /// The language's block comment of this index.
    Comment(u8),
    String(u8),
    /// Inside a tag, between its attributes; or in an attribute's value, its quote.
    Tag,
    TagString(u8),
    /// A Markdown code block.
    Fence,
}

pub type Spans = Vec<(Range<usize>, Kind)>;

#[derive(Clone, Copy)]
struct Quote {
    open: &'static str,
    close: &'static str,
    /// Goes on past the line's end.
    lines: bool,
    escapes: bool,
}

const fn q(open: &'static str, close: &'static str, lines: bool, escapes: bool) -> Quote {
    Quote { open, close, lines, escapes }
}

#[derive(Clone, Copy)]
struct Code {
    keywords: &'static str,
    line: &'static [&'static str],
    block: &'static [(&'static str, &'static str)],
    strings: &'static [Quote],
    /// `'` is a character, closed at once (Rust's lifetimes are none).
    chars: bool,
    /// SQL's keywords, in any case.
    any_case: bool,
    /// What a keyword may start with besides letters and `_`: `#include`, `@media`, `.PHONY`.
    starts: &'static str,
    /// A line's key before this: YAML's `name:`, TOML's `name =`.
    key: Option<char>,
    /// TOML's `[section]` lines.
    sections: bool,
}

const CODE: Code = Code {
    keywords: "",
    line: &[],
    block: &[],
    strings: const { &[] },
    chars: false,
    any_case: false,
    starts: "",
    key: None,
    sections: false,
};

#[derive(Clone, Copy)]
enum Rules {
    Code(Code),
    Markup,
    Markdown,
    Diff,
}

const C_LIKE: &[(&str, &str)] = &[("/*", "*/")];
const SLASHES: &[&str] = &["//"];
const HASH: &[&str] = &["#"];
const DOUBLE: &[Quote] = &[q("\"", "\"", false, true), q("'", "'", false, true)];
const DOUBLE_LINES: &[Quote] = &[q("\"", "\"", true, true), q("'", "'", false, true)];

macro_rules! c_words {
    () => {
        "auto break case char const continue default do double else enum extern float for goto if inline int long \
         register restrict return short signed sizeof static struct switch typedef union unsigned void volatile while \
         bool true false NULL #include #define #undef #if #ifdef #ifndef #elif #else #endif #pragma #error"
    };
}

macro_rules! js_words {
    () => {
        "break case catch class const continue debugger default delete do else export extends finally for function \
         if import in instanceof let new return super switch this throw try typeof var void while with yield async \
         await of static get set from true false null undefined"
    };
}

const TS_WORDS: &str = concat!(
    js_words!(),
    " interface type enum implements namespace declare abstract readonly private protected public as is keyof \
     infer never unknown any string number boolean symbol bigint object satisfies"
);

/// The rules for `text_edit::language_of`'s language.
fn rules(language: &str) -> Option<Rules> {
    let code = |code: Code| Some(Rules::Code(code));
    match language {
        "c" => code(Code { keywords: c_words!(), line: SLASHES, block: C_LIKE, strings: DOUBLE, chars: true, starts: "#", ..CODE }),
        "cpp" => code(Code {
            keywords: concat!(
                c_words!(),
                " alignas alignof asm catch class constexpr const_cast decltype delete dynamic_cast explicit export \
                 friend mutable namespace new noexcept nullptr operator private protected public reinterpret_cast \
                 static_assert static_cast template this throw try typeid typename using virtual override final \
                 concept requires co_await co_return co_yield consteval constinit wchar_t char8_t char16_t char32_t"
            ),
            line: SLASHES,
            block: C_LIKE,
            strings: DOUBLE,
            chars: true,
            starts: "#",
            ..CODE
        }),
        "csharp" => code(Code {
            keywords: "abstract as base bool break byte case catch char checked class const continue decimal default \
                       delegate do double else enum event explicit extern false finally fixed float for foreach goto if \
                       implicit in int interface internal is lock long namespace new null object operator out override \
                       params private protected public readonly ref return sbyte sealed short sizeof stackalloc static \
                       string struct switch this throw true try typeof uint ulong unchecked unsafe ushort using virtual \
                       void volatile while var async await get set value yield record init where nameof dynamic #if \
                       #else #elif #endif #region #endregion #define #pragma #nullable",
            line: SLASHES,
            block: C_LIKE,
            strings: DOUBLE,
            chars: true,
            starts: "#",
            ..CODE
        }),
        "java" => code(Code {
            keywords: "abstract assert boolean break byte case catch char class const continue default do double else \
                       enum extends final finally float for goto if implements import instanceof int interface long \
                       native new package private protected public return short static strictfp super switch \
                       synchronized this throw throws transient try void volatile while true false null var record \
                       sealed permits yield",
            line: SLASHES,
            block: C_LIKE,
            strings: const { &[q("\"\"\"", "\"\"\"", true, true), q("\"", "\"", false, true), q("'", "'", false, true)] },
            chars: true,
            ..CODE
        }),
        "javascript" => code(Code { keywords: js_words!(), line: SLASHES, block: C_LIKE, strings: JS_STRINGS, ..CODE }),
        "typescript" | "tsx" => code(Code { keywords: TS_WORDS, line: SLASHES, block: C_LIKE, strings: JS_STRINGS, ..CODE }),
        "go" => code(Code {
            keywords: "break case chan const continue default defer else fallthrough for func go goto if import \
                       interface map package range return select struct switch type var true false nil iota bool byte \
                       rune string int int8 int16 int32 int64 uint uint8 uint16 uint32 uint64 uintptr float32 float64 \
                       complex64 complex128 error any make new len cap append",
            line: SLASHES,
            block: C_LIKE,
            strings: const { &[q("\"", "\"", false, true), q("`", "`", true, false), q("'", "'", false, true)] },
            chars: true,
            ..CODE
        }),
        "rust" => code(Code {
            keywords: "as async await break const continue crate dyn else enum extern false fn for if impl in let loop \
                       match mod move mut pub ref return self Self static struct super trait true type unsafe use where \
                       while union macro_rules bool char str u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64 \
                       String Vec Option Some None Result Ok Err Box",
            line: SLASHES,
            block: C_LIKE,
            strings: const { &[q("\"", "\"", true, true), q("'", "'", false, true)] },
            chars: true,
            ..CODE
        }),
        "python" => code(Code {
            keywords: "False None True and as assert async await break class continue def del elif else except finally \
                       for from global if import in is lambda nonlocal not or pass raise return try while with yield \
                       match case self",
            line: HASH,
            strings: const { &[
                q("\"\"\"", "\"\"\"", true, true),
                q("'''", "'''", true, true),
                q("\"", "\"", false, true),
                q("'", "'", false, true),
            ] },
            ..CODE
        }),
        "ruby" => code(Code {
            keywords: "BEGIN END alias and begin break case class def defined? do else elsif end ensure false for if in \
                       module next nil not or redo rescue retry return self super then true undef unless until when \
                       while yield require attr_accessor attr_reader attr_writer",
            line: HASH,
            block: &[("=begin", "=end")],
            strings: DOUBLE_LINES,
            ..CODE
        }),
        "php" => code(Code {
            keywords: "abstract and array as break callable case catch class clone const continue declare default do \
                       echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile extends final \
                       finally fn for foreach function global goto if implements include include_once instanceof \
                       insteadof interface isset list match namespace new or print private protected public readonly \
                       require require_once return static switch throw trait try unset use var while xor yield true \
                       false null",
            line: &["//", "#"],
            block: C_LIKE,
            strings: DOUBLE_LINES,
            ..CODE
        }),
        "lua" => code(Code {
            keywords: "and break do else elseif end false for function goto if in local nil not or repeat return then \
                       true until while",
            line: &["--"],
            block: &[("--[[", "]]")],
            strings: const { &[q("[[", "]]", true, false), q("\"", "\"", false, true), q("'", "'", false, true)] },
            ..CODE
        }),
        "bash" => code(Code {
            keywords: "if then else elif fi case esac for while until do done in function select time return exit \
                       break continue local export readonly declare unset shift source alias echo printf read cd test \
                       eval exec set trap true false",
            line: HASH,
            strings: const { &[q("\"", "\"", true, true), q("'", "'", true, false)] },
            ..CODE
        }),
        "kotlin" => code(Code {
            keywords: "as break class continue do else false for fun if in interface is null object package return super \
                       this throw true try typealias typeof val var when while by catch constructor finally get import \
                       init set where abstract annotation companion const crossinline data enum expect external final \
                       infix inline inner internal lateinit noinline open operator out override private protected public \
                       reified sealed suspend tailrec vararg",
            line: SLASHES,
            block: C_LIKE,
            strings: const { &[q("\"\"\"", "\"\"\"", true, false), q("\"", "\"", false, true), q("'", "'", false, true)] },
            chars: true,
            ..CODE
        }),
        "scala" => code(Code {
            keywords: "abstract case catch class def do else extends false final finally for forSome if implicit import \
                       lazy match new null object override package private protected return sealed super this throw \
                       trait try true type val var while with yield given using enum then export",
            line: SLASHES,
            block: C_LIKE,
            strings: const { &[q("\"\"\"", "\"\"\"", true, false), q("\"", "\"", false, true), q("'", "'", false, true)] },
            chars: true,
            ..CODE
        }),
        "swift" => code(Code {
            keywords: "associatedtype class deinit enum extension fileprivate func import init inout internal let open \
                       operator private protocol public rethrows static struct subscript typealias var break case \
                       continue default defer do else fallthrough for guard if in repeat return switch where while as \
                       catch false is nil super self Self throw throws true try async await some any",
            line: SLASHES,
            block: C_LIKE,
            strings: const { &[q("\"\"\"", "\"\"\"", true, true), q("\"", "\"", false, true)] },
            ..CODE
        }),
        "zig" => code(Code {
            keywords: "addrspace align allowzero and anyframe anytype asm async await break callconv catch comptime const \
                       continue defer else enum errdefer error export extern fn for if inline linksection noalias \
                       noinline nosuspend opaque or orelse packed pub resume return struct suspend switch test \
                       threadlocal try union unreachable usingnamespace var volatile while true false null undefined",
            line: SLASHES,
            strings: DOUBLE,
            chars: true,
            ..CODE
        }),
        "elixir" => code(Code {
            keywords: "def defp defmodule defmacro defstruct defprotocol defimpl do end fn if else unless case cond when \
                       with for receive try catch rescue after raise import alias require use quote unquote true false \
                       nil and or not in",
            line: HASH,
            strings: const { &[q("\"\"\"", "\"\"\"", true, true), q("\"", "\"", true, true), q("'", "'", false, true)] },
            ..CODE
        }),
        "sql" => code(Code {
            keywords: "select from where and or not insert into values update set delete create table drop alter add \
                       column index view join inner left right outer full cross on group by order having limit offset \
                       as distinct union all case when then else end null is in like between exists primary key foreign \
                       references default unique check constraint begin commit rollback transaction with returning \
                       varchar char text integer int bigint smallint boolean date timestamp numeric decimal float real \
                       serial asc desc if replace trigger function procedure true false",
            line: &["--"],
            block: C_LIKE,
            strings: const { &[q("'", "'", true, false), q("\"", "\"", false, false)] },
            any_case: true,
            ..CODE
        }),
        "graphql" => code(Code {
            keywords: "query mutation subscription fragment on type interface union enum input scalar schema extend \
                       directive implements true false null",
            line: HASH,
            strings: const { &[q("\"\"\"", "\"\"\"", true, true), q("\"", "\"", false, true)] },
            ..CODE
        }),
        "proto" => code(Code {
            keywords: "syntax package import option message enum service rpc returns stream repeated optional required \
                       map oneof reserved extend true false double float int32 int64 uint32 uint64 sint32 sint64 fixed32 \
                       fixed64 sfixed32 sfixed64 bool string bytes",
            line: SLASHES,
            block: C_LIKE,
            strings: DOUBLE,
            ..CODE
        }),
        "json" => code(Code { keywords: "true false null", line: SLASHES, block: C_LIKE, strings: const { &[q("\"", "\"", false, true)] }, ..CODE }),
        "yaml" => code(Code { keywords: "true false null yes no on off", line: HASH, strings: DOUBLE, key: Some(':'), ..CODE }),
        "toml" => code(Code {
            keywords: "true false",
            line: HASH,
            strings: const { &[
                q("\"\"\"", "\"\"\"", true, true),
                q("'''", "'''", true, false),
                q("\"", "\"", false, true),
                q("'", "'", false, false),
            ] },
            key: Some('='),
            sections: true,
            ..CODE
        }),
        "css" => code(Code {
            keywords: "@media @import @font-face @keyframes @supports @charset @page @layer @container @use @mixin \
                       @include @extend !important",
            block: C_LIKE,
            line: SLASHES,
            strings: DOUBLE,
            starts: "@!",
            key: Some(':'),
            ..CODE
        }),
        "make" => code(Code {
            keywords: "ifeq ifneq ifdef ifndef else endif define endef include override export unexport vpath .PHONY \
                       .SUFFIXES .DEFAULT .PRECIOUS .INTERMEDIATE .SECONDARY .DELETE_ON_ERROR",
            line: HASH,
            strings: DOUBLE,
            starts: ".",
            key: Some(':'),
            ..CODE
        }),
        "cmake" => code(Code {
            keywords: "if else elseif endif foreach endforeach while endwhile function endfunction macro endmacro set \
                       unset project add_executable add_library target_link_libraries target_include_directories \
                       target_compile_options target_compile_definitions include_directories find_package \
                       cmake_minimum_required option message install add_subdirectory return include list string",
            line: HASH,
            strings: const { &[q("\"", "\"", true, true)] },
            any_case: true,
            ..CODE
        }),
        "html" | "xml" | "svelte" | "astro" | "ejs" | "erb" => Some(Rules::Markup),
        "markdown" => Some(Rules::Markdown),
        "diff" => Some(Rules::Diff),
        _ => None,
    }
}

const JS_STRINGS: &[Quote] = &[q("`", "`", true, true), q("\"", "\"", false, true), q("'", "'", false, true)];

/// A language's rules: by `text_edit::language_of`'s names; none for plain text.
#[derive(Clone, Copy)]
pub struct Syntax(Rules);

impl Syntax {
    pub fn of(language: &str) -> Option<Syntax> {
        rules(language).map(Syntax)
    }

    /// The line's coloured parts, in order, and what it leaves open.
    pub fn line(&self, text: &str, open: Open) -> (Spans, Open) {
        let mut spans = Vec::new();
        let open = self.scan(text, open, Some(&mut spans));
        (spans, open)
    }

    /// Only what the line leaves open; no keywords looked up.
    pub fn next(&self, text: &str, open: Open) -> Open {
        self.scan(text, open, None)
    }

    fn scan(&self, text: &str, open: Open, spans: Option<&mut Spans>) -> Open {
        let mut out = Out(spans);
        match &self.0 {
            Rules::Code(code) => scan_code(code, text, open, &mut out),
            Rules::Markup => scan_markup(text, open, &mut out),
            Rules::Markdown => scan_markdown(text, open, &mut out),
            Rules::Diff => {
                let kind = match text.as_bytes().first() {
                    _ if text.starts_with("+++") || text.starts_with("---") => Some(Kind::Markup),
                    Some(b'+') => Some(Kind::Added),
                    Some(b'-') => Some(Kind::Removed),
                    Some(b'@') => Some(Kind::Keyword),
                    _ => None,
                };
                if let Some(kind) = kind {
                    out.push(0..text.len(), kind);
                }
                Open::Nothing
            }
        }
    }
}

/// A whole text in its colours, as the viewer shows a file: where each line
/// starts, and what it starts with open.
pub struct Coloured {
    syntax: Syntax,
    text: String,
    starts: Vec<usize>,
    opens: Vec<Open>,
}

impl Coloured {
    pub fn new(syntax: Syntax, text: String) -> Self {
        let ends = memchr::memchr_iter(b'\n', text.as_bytes()).map(|at| at + 1);
        let starts: Vec<usize> = std::iter::once(0).chain(ends).collect();
        let mut coloured = Coloured { syntax, text, opens: Vec::with_capacity(starts.len()), starts };
        let mut open = Open::Nothing;
        for ix in 0..coloured.starts.len() {
            coloured.opens.push(open);
            open = syntax.next(coloured.line(ix).1, open);
        }
        coloured
    }

    /// Line `ix`'s start, and its text without its line end.
    fn line(&self, ix: usize) -> (usize, &str) {
        let start = self.starts[ix];
        let end = self.starts.get(ix + 1).map_or(self.text.len(), |next| next - 1);
        let line = &self.text[start..end];
        (start, line.strip_suffix('\r').unwrap_or(line))
    }

    /// The coloured parts of the lines `range` touches, by their place in the text.
    pub fn spans(&self, range: Range<usize>) -> Spans {
        let first = self.starts.partition_point(|&start| start <= range.start).saturating_sub(1);
        let mut spans = Vec::new();
        for ix in (first..self.starts.len()).take_while(|&ix| ix == first || self.starts[ix] < range.end) {
            let (start, line) = self.line(ix);
            let found = self.syntax.line(line, self.opens[ix]).0;
            spans.extend(found.into_iter().map(|(at, kind)| (start + at.start..start + at.end, kind)));
        }
        spans
    }
}

/// Where the spans go, when they are wanted.
struct Out<'a>(Option<&'a mut Spans>);

impl Out<'_> {
    fn push(&mut self, range: Range<usize>, kind: Kind) {
        if let Some(spans) = self.0.as_mut().filter(|_| !range.is_empty()) {
            spans.push((range, kind));
        }
    }

    fn wanted(&self) -> bool {
        self.0.is_some()
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where `quote` closes in `text` from `at`, past it.
fn closing(text: &str, at: usize, quote: &Quote) -> Option<usize> {
    let mut chars = text[at..].char_indices();
    while let Some((ix, c)) = chars.next() {
        if quote.escapes && c == '\\' {
            chars.next();
        } else if text[at + ix..].starts_with(quote.close) {
            return Some(at + ix + quote.close.len());
        }
    }
    None
}

fn scan_code(code: &Code, text: &str, mut open: Open, out: &mut Out) -> Open {
    let (mut at, mut from) = (0, 0);
    if open == Open::Nothing {
        let trimmed = text.trim_start();
        let indent = text.len() - trimmed.len();
        if code.sections && trimmed.starts_with('[') {
            out.push(indent..text.len(), Kind::Markup);
            return Open::Nothing;
        }
        if let Some(separator) = code.key {
            let item = trimmed.strip_prefix("- ").unwrap_or(trimmed);
            let start = text.len() - item.len();
            let key = item.find(|c: char| !(is_word(c) || "-. \"'".contains(c))).filter(|&end| end > 0);
            let after = |end: usize| item[end + 1..].chars().next().is_none_or(|c| c.is_whitespace() || separator == '=');
            if let Some(end) = key.filter(|&end| item[end..].starts_with(separator) && after(end)) {
                out.push(start..start + item[..end].trim_end().len(), Kind::Keyword);
                at = start + end;
            }
        }
    }
    loop {
        match open {
            Open::Comment(ix) => {
                let close = code.block[ix as usize].1;
                match text[at..].find(close) {
                    Some(end) => {
                        let end = at + end + close.len();
                        out.push(from..end, Kind::Comment);
                        (at, open) = (end, Open::Nothing);
                    }
                    None => {
                        out.push(from..text.len(), Kind::Comment);
                        return open;
                    }
                }
            }
            Open::String(ix) => {
                let quote = code.strings[ix as usize];
                match closing(text, at, &quote) {
                    Some(end) => {
                        out.push(from..end, Kind::String);
                        (at, open) = (end, Open::Nothing);
                    }
                    None => {
                        out.push(from..text.len(), Kind::String);
                        return if quote.lines { open } else { Open::Nothing };
                    }
                }
            }
            _ => {
                let rest = &text[at..];
                let Some(c) = rest.chars().next() else { return Open::Nothing };
                // `#` comments only where a word starts: not `$#`, nor `a#b`.
                let spaced = text[..at].chars().next_back().is_none_or(char::is_whitespace);
                if code.line.iter().any(|line| rest.starts_with(line) && (*line != "#" || spaced)) {
                    out.push(at..text.len(), Kind::Comment);
                    return Open::Nothing;
                }
                if let Some(ix) = code.block.iter().position(|(start, _)| rest.starts_with(start)) {
                    (from, at, open) = (at, at + code.block[ix].0.len(), Open::Comment(ix as u8));
                    continue;
                }
                if let Some(ix) = code.strings.iter().position(|quote| rest.starts_with(quote.open)) {
                    let quote = code.strings[ix];
                    // A character: one, or an escape, then its quote.
                    let character = |rest: &str| {
                        let mut chars = rest[1..].chars();
                        match chars.next() {
                            Some('\\') => rest[1..].char_indices().skip(2).take(10).any(|(_, c)| c == '\''),
                            Some(_) => chars.next() == Some('\''),
                            None => false,
                        }
                    };
                    if quote.open == "'" && code.chars && !character(rest) {
                        at += 1;
                        continue;
                    }
                    (from, at, open) = (at, at + quote.open.len(), Open::String(ix as u8));
                    continue;
                }
                let word_start = |c: char| c.is_alphabetic() || c == '_' || code.starts.contains(c);
                let digits = c.is_ascii_digit();
                if digits || word_start(c) {
                    // Numbers as 0x1F, 1_000u32, 2.5.
                    let tail = &rest[c.len_utf8()..];
                    let end = at + c.len_utf8() + tail.find(|c: char| !(is_word(c) || digits && c == '.')).unwrap_or(tail.len());
                    let word = &text[at..end];
                    if digits {
                        out.push(at..end, Kind::Number);
                    } else if out.wanted() && is_keyword(code, word) {
                        out.push(at..end, Kind::Keyword);
                    }
                    at = end;
                    continue;
                }
                at += c.len_utf8();
            }
        }
    }
}

fn is_keyword(code: &Code, word: &str) -> bool {
    let mut keywords = code.keywords.split_ascii_whitespace();
    match code.any_case {
        true => keywords.any(|keyword| keyword.eq_ignore_ascii_case(word)),
        false => keywords.any(|keyword| keyword == word),
    }
}

/// HTML and XML: tags, their attributes and values, comments.
fn scan_markup(text: &str, mut open: Open, out: &mut Out) -> Open {
    let (mut at, mut from) = (0, 0);
    loop {
        let rest = &text[at..];
        match open {
            Open::Comment(_) => match rest.find("-->") {
                Some(end) => {
                    out.push(from..at + end + 3, Kind::Comment);
                    (at, open) = (at + end + 3, Open::Nothing);
                }
                None => {
                    out.push(from..text.len(), Kind::Comment);
                    return open;
                }
            },
            Open::TagString(quote) => match rest.find(quote as char) {
                Some(end) => {
                    out.push(from..at + end + 1, Kind::String);
                    (at, open) = (at + end + 1, Open::Tag);
                }
                None => {
                    out.push(from..text.len(), Kind::String);
                    return open;
                }
            },
            Open::Tag => {
                let Some(c) = rest.chars().next() else { return open };
                if rest.starts_with("/>") || rest.starts_with("?>") || c == '>' {
                    let end = at + if c == '>' { 1 } else { 2 };
                    out.push(at..end, Kind::Markup);
                    (at, open) = (end, Open::Nothing);
                } else if c == '"' || c == '\'' {
                    (from, at, open) = (at, at + 1, Open::TagString(c as u8));
                } else if is_word(c) {
                    let end = at + rest.find(|c: char| !(is_word(c) || "-:.".contains(c))).unwrap_or(rest.len());
                    out.push(at..end, Kind::Keyword);
                    at = end;
                } else {
                    at += c.len_utf8();
                }
            }
            _ => {
                let Some(start) = rest.find('<') else { return Open::Nothing };
                let start = at + start;
                if text[start..].starts_with("<!--") {
                    (from, at, open) = (start, start + 4, Open::Comment(0));
                    continue;
                }
                let name = &text[start + 1..];
                let length = name.find(|c: char| !(is_word(c) || "/!?-:.".contains(c))).unwrap_or(name.len());
                if length == 0 {
                    at = start + 1;
                    continue;
                }
                out.push(start..start + 1 + length, Kind::Markup);
                (at, open) = (start + 1 + length, Open::Tag);
            }
        }
    }
}

/// Markdown: headings, quotes, code blocks and `code`.
fn scan_markdown(text: &str, open: Open, out: &mut Out) -> Open {
    let trimmed = text.trim_start();
    let fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
    if open == Open::Fence || fence {
        out.push(0..text.len(), Kind::String);
        return if (open == Open::Fence) != fence { Open::Fence } else { Open::Nothing };
    }
    if trimmed.starts_with('#') {
        out.push(0..text.len(), Kind::Markup);
    } else if trimmed.starts_with('>') {
        out.push(0..text.len(), Kind::Comment);
    } else if out.wanted() {
        let mut at = 0;
        while let Some(start) = text[at..].find('`') {
            let start = at + start;
            let Some(end) = text[start + 1..].find('`') else { break };
            out.push(start..start + end + 2, Kind::String);
            at = start + end + 2;
        }
    }
    Open::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each line's coloured parts as text, with their kinds.
    fn colours(language: &str, text: &str) -> Vec<(std::string::String, Kind)> {
        let syntax = Syntax::of(language).unwrap();
        let mut open = Open::Nothing;
        let mut found = Vec::new();
        for line in text.split('\n') {
            let (spans, next) = syntax.line(line, open);
            assert_eq!(syntax.next(line, open), next, "{line}");
            found.extend(spans.into_iter().map(|(range, kind)| (line[range].to_owned(), kind)));
            open = next;
        }
        found
    }

    fn of(pairs: &[(&str, Kind)]) -> Vec<(std::string::String, Kind)> {
        pairs.iter().map(|(text, kind)| (text.to_string(), *kind)).collect()
    }

    use Kind::*;

    #[test]
    fn code_by_its_rules() {
        let rust = "fn f<'a>(x: &'a str) -> char { 'x' } // done\nlet s = \"a \\\" /* b\nc\"; /* one\ntwo */ 0x1F";
        assert_eq!(
            colours("rust", rust),
            of(&[
                ("fn", Keyword),
                ("str", Keyword),
                ("char", Keyword),
                ("'x'", String),
                ("// done", Comment),
                ("let", Keyword),
                ("\"a \\\" /* b", String),
                ("c\"", String),
                ("/* one", Comment),
                ("two */", Comment),
                ("0x1F", Number),
            ])
        );
        let python = "def f():  # it's\n    '''doc\n    more''' if x else 'y'";
        assert_eq!(
            colours("python", python),
            of(&[("def", Keyword), ("# it's", Comment), ("'''doc", String), ("    more'''", String), ("if", Keyword), ("else", Keyword), ("'y'", String)])
        );
        assert_eq!(colours("bash", "echo $# a#b # c"), of(&[("echo", Keyword), ("# c", Comment)]));
        assert_eq!(colours("sql", "SELECT name FROM t -- all"), of(&[("SELECT", Keyword), ("FROM", Keyword), ("-- all", Comment)]));
        assert_eq!(colours("c", "#include <a.h>"), of(&[("#include", Keyword)]));
    }

    #[test]
    fn a_whole_text_by_its_places() {
        let coloured = Coloured::new(Syntax::of("c").unwrap(), "int a; /* x\r\ny */ b\nint".into());
        assert_eq!(coloured.spans(9..14), [(0..3, Keyword), (7..11, Comment), (13..17, Comment)]);
        assert_eq!(coloured.spans(20..21), [(20..23, Keyword)]);
    }

    #[test]
    fn keys_sections_markup_markdown_and_diffs() {
        assert_eq!(colours("yaml", "- name: x # n\nurl: http://a#b"), of(&[("name", Keyword), ("# n", Comment), ("url", Keyword)]));
        assert_eq!(colours("toml", "[package]\nname = \"sc\""), of(&[("[package]", Markup), ("name", Keyword), ("\"sc\"", String)]));
        let html = "<a href=\"x\n y\">don't</a><!-- c\n-->";
        assert_eq!(
            colours("html", html),
            of(&[("<a", Markup), ("href", Keyword), ("\"x", String), (" y\"", String), (">", Markup), ("</a", Markup), (">", Markup), ("<!-- c", Comment), ("-->", Comment)])
        );
        let markdown = "# Title\nuse `x`\n```\n# not a title\n```";
        assert_eq!(
            colours("markdown", markdown),
            of(&[("# Title", Markup), ("`x`", String), ("```", String), ("# not a title", String), ("```", String)])
        );
        assert_eq!(colours("diff", "@@ -1 +1 @@\n-a\n+b\n c"), of(&[("@@ -1 +1 @@", Keyword), ("-a", Removed), ("+b", Added)]));
    }
}
