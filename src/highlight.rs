//! Small syntax highlighter for code blocks: comments, strings, numbers,
//! keywords, calls and types for the languages a physics group pastes
//! (Python, C/C++, shell, Rust, Go, JS/TS, JSON/YAML/TOML, SQL, diffs).
//! A tokenizer, not a parser: cheap enough to run on every layout.

use crate::theme::Theme;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};

#[derive(Clone, Copy, PartialEq)]
enum Family {
    CLike,
    Python,
    Shell,
    Data,
    Sql,
    Diff,
    Plain,
}

struct Lang {
    family: Family,
    keywords: &'static [&'static str],
}

const C_KW: &[&str] = &[
    "auto", "break", "case", "catch", "class", "const", "constexpr", "continue", "default", "delete", "do", "else",
    "enum", "explicit", "extern", "false", "for", "friend", "goto", "if", "inline", "namespace", "new", "nullptr",
    "operator", "private", "protected", "public", "return", "sizeof", "static", "struct", "switch", "template",
    "this", "throw", "true", "try", "typedef", "typename", "union", "using", "virtual", "void", "volatile", "while",
    "int", "float", "double", "char", "bool", "long", "short", "unsigned", "signed", "size_t", "std", "override",
    "final", "#include", "#define", "#ifdef", "#ifndef", "#endif", "#pragma", "#if", "#else",
];
const RUST_KW: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
    "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
];
const GO_KW: &[&str] = &[
    "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "for", "func", "go",
    "goto", "if", "import", "interface", "map", "package", "range", "return", "select", "struct", "switch", "type",
    "var", "nil", "true", "false",
];
const JS_KW: &[&str] = &[
    "async", "await", "break", "case", "catch", "class", "const", "continue", "default", "delete", "do", "else",
    "export", "extends", "false", "finally", "for", "function", "if", "import", "in", "instanceof", "let", "new",
    "null", "return", "super", "switch", "this", "throw", "true", "try", "typeof", "undefined", "var", "void",
    "while", "yield", "interface", "type", "public", "private", "static", "final", "void", "int", "boolean",
];
const PY_KW: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
    "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal",
    "not", "or", "pass", "raise", "return", "try", "while", "with", "yield", "self", "print",
];
const SH_KW: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function",
    "return", "export", "local", "source", "alias", "unset", "set", "echo", "cd", "exit",
];
const SQL_KW: &[&str] = &[
    "select", "from", "where", "and", "or", "not", "insert", "into", "values", "update", "set", "delete", "create",
    "table", "drop", "alter", "join", "left", "right", "inner", "outer", "on", "group", "by", "order", "having",
    "limit", "as", "distinct", "null", "is", "in", "like", "count", "sum", "avg", "max", "min",
];
const DATA_KW: &[&str] = &["true", "false", "null", "yes", "no", "on", "off"];

fn lang(tag: &str) -> Lang {
    let tag = tag.trim().to_ascii_lowercase();
    let (family, keywords): (Family, &'static [&'static str]) = match tag.as_str() {
        "c" | "h" | "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hh" | "cuda" | "cu" | "objc" => (Family::CLike, C_KW),
        "rust" | "rs" => (Family::CLike, RUST_KW),
        "go" | "golang" => (Family::CLike, GO_KW),
        "js" | "javascript" | "ts" | "typescript" | "jsx" | "tsx" | "java" | "kotlin" | "kt" | "scala" | "cs"
        | "csharp" | "swift" | "dart" => (Family::CLike, JS_KW),
        "py" | "python" | "python3" | "ipython" | "pycon" => (Family::Python, PY_KW),
        "sh" | "bash" | "zsh" | "shell" | "console" | "shellsession" | "fish" | "ksh" | "tcsh" | "csh" => {
            (Family::Shell, SH_KW)
        }
        "json" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "jsonc" => (Family::Data, DATA_KW),
        "sql" | "mysql" | "postgresql" | "sqlite" => (Family::Sql, SQL_KW),
        "diff" | "patch" => (Family::Diff, &[]),
        _ => (Family::Plain, &[]),
    };
    Lang { family, keywords }
}

struct Palette {
    text: Color32,
    keyword: Color32,
    string: Color32,
    comment: Color32,
    number: Color32,
    func: Color32,
    ty: Color32,
    added: Color32,
    removed: Color32,
}

fn palette(theme: &Theme) -> Palette {
    // One Light / One Dark.
    if theme.dark {
        Palette {
            text: Color32::from_rgb(0xab, 0xb2, 0xbf),
            keyword: Color32::from_rgb(0xc6, 0x78, 0xdd),
            string: Color32::from_rgb(0x98, 0xc3, 0x79),
            comment: Color32::from_rgb(0x7f, 0x84, 0x8e),
            number: Color32::from_rgb(0xd1, 0x9a, 0x66),
            func: Color32::from_rgb(0x61, 0xaf, 0xef),
            ty: Color32::from_rgb(0xe5, 0xc0, 0x7b),
            added: Color32::from_rgb(0x98, 0xc3, 0x79),
            removed: Color32::from_rgb(0xe0, 0x6c, 0x75),
        }
    } else {
        Palette {
            text: Color32::from_rgb(0x38, 0x3a, 0x42),
            keyword: Color32::from_rgb(0xa6, 0x26, 0xa4),
            string: Color32::from_rgb(0x50, 0xa1, 0x4f),
            comment: Color32::from_rgb(0x8f, 0x91, 0x99),
            number: Color32::from_rgb(0x98, 0x68, 0x01),
            func: Color32::from_rgb(0x40, 0x78, 0xf2),
            ty: Color32::from_rgb(0xc1, 0x84, 0x01),
            added: Color32::from_rgb(0x2e, 0x8b, 0x3e),
            removed: Color32::from_rgb(0xca, 0x12, 0x43),
        }
    }
}

/// The code as a monospace job, coloured for `lang` (plain when unknown).
pub fn job(code: &str, lang_tag: &str, theme: &Theme, size: f32) -> LayoutJob {
    let lang = lang(lang_tag);
    let p = palette(theme);
    let font = FontId::monospace(size);
    let mut job = LayoutJob::default();
    let mut push = |text: &str, color: Color32, italic: bool| {
        if !text.is_empty() {
            job.append(text, 0.0, TextFormat { font_id: font.clone(), color, italics: italic, ..Default::default() });
        }
    };
    if lang.family == Family::Plain {
        push(code, p.text, false);
        return job;
    }
    if lang.family == Family::Diff {
        for line in code.split_inclusive('\n') {
            let color = match line.chars().next() {
                Some('+') if !line.starts_with("+++") => p.added,
                Some('-') if !line.starts_with("---") => p.removed,
                Some('@') => p.func,
                _ => p.text,
            };
            push(line, color, false);
        }
        return job;
    }

    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut line_start = true;
    let mut command_pos = true; // shell: next word is a command
    let text_of = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    while i < n {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        // Comments.
        let comment_here = match lang.family {
            Family::CLike => c == '/' && (next == Some('/') || next == Some('*')),
            Family::Python | Family::Data => c == '#' || (c == ';' && line_start && lang_tag_is(lang_tag, &["ini", "cfg"])),
            Family::Shell => c == '#' && (i == 0 || chars[i - 1].is_whitespace()),
            Family::Sql => c == '-' && next == Some('-'),
            _ => false,
        };
        if comment_here {
            let start = i;
            if lang.family == Family::CLike && next == Some('*') {
                i += 2;
                while i < n && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i = (i + 2).min(n);
            } else {
                while i < n && chars[i] != '\n' {
                    i += 1;
                }
            }
            push(&text_of(start, i), p.comment, true);
            continue;
        }
        // Strings.
        if c == '"' || c == '\'' || (c == '`' && matches!(lang.family, Family::CLike | Family::Shell)) {
            let start = i;
            let triple = lang.family == Family::Python && next == Some(c) && chars.get(i + 2) == Some(&c);
            i += if triple { 3 } else { 1 };
            while i < n {
                if chars[i] == '\\' && c != '\'' || (chars[i] == '\\' && lang.family != Family::Shell) {
                    i += 2;
                    continue;
                }
                if chars[i] == c && (!triple || (chars.get(i + 1) == Some(&c) && chars.get(i + 2) == Some(&c))) {
                    i += if triple { 3 } else { 1 };
                    break;
                }
                if chars[i] == '\n' && !triple && c != '`' {
                    break;
                }
                i += 1;
            }
            let i2 = i.min(n);
            // JSON / YAML keys: a string followed by a colon.
            let mut j = i2;
            while j < n && chars[j] == ' ' {
                j += 1;
            }
            let color = if lang.family == Family::Data && chars.get(j) == Some(&':') { p.keyword } else { p.string };
            push(&text_of(start, i2), color, false);
            i = i2;
            line_start = false;
            command_pos = false;
            continue;
        }
        // Numbers.
        if c.is_ascii_digit() || (c == '.' && next.is_some_and(|d| d.is_ascii_digit()) && (i == 0 || !chars[i - 1].is_alphanumeric())) {
            let start = i;
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            push(&text_of(start, i), p.number, false);
            line_start = false;
            command_pos = false;
            continue;
        }
        // Shell variables and flags.
        if lang.family == Family::Shell && c == '$' {
            let start = i;
            i += 1;
            if i < n && chars[i] == '{' {
                while i < n && chars[i] != '}' {
                    i += 1;
                }
                i = (i + 1).min(n);
            } else {
                while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
            }
            push(&text_of(start, i), p.ty, false);
            continue;
        }
        if lang.family == Family::Shell && c == '-' && (i == 0 || chars[i - 1].is_whitespace()) && !command_pos {
            let start = i;
            while i < n && !chars[i].is_whitespace() && chars[i] != '=' {
                i += 1;
            }
            push(&text_of(start, i), p.number, false);
            continue;
        }
        // Words.
        if c.is_alphabetic() || c == '_' || (c == '#' && lang.family == Family::CLike) || (c == '@' && lang.family == Family::Python) {
            let start = i;
            i += 1;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_' || (lang.family == Family::Shell && matches!(chars[i], '-' | '.' | '/'))) {
                i += 1;
            }
            let word = text_of(start, i);
            let mut k = i;
            while k < n && chars[k] == ' ' {
                k += 1;
            }
            let call = chars.get(k) == Some(&'(');
            let lower = word.to_ascii_lowercase();
            let is_kw = if lang.family == Family::Sql { lang.keywords.contains(&lower.as_str()) } else { lang.keywords.contains(&word.as_str()) };
            let color = if is_kw {
                p.keyword
            } else if word.starts_with('@') || (lang.family == Family::Shell && command_pos) {
                p.func
            } else if lang.family == Family::Data && line_start && chars.get(k).is_some_and(|c| *c == ':' || *c == '=') {
                p.keyword
            } else if call {
                p.func
            } else if lang.family == Family::CLike && word.chars().next().is_some_and(char::is_uppercase) {
                p.ty
            } else {
                p.text
            };
            push(&word, color, false);
            line_start = false;
            command_pos = false;
            continue;
        }
        // Everything else, one char at a time (newlines reset line state).
        if c == '\n' {
            line_start = true;
            command_pos = true;
        } else if lang.family == Family::Shell && matches!(c, '|' | ';' | '&') {
            command_pos = true;
        } else if !c.is_whitespace() {
            line_start = false;
        }
        push(&c.to_string(), p.text, false);
        i += 1;
    }
    job
}

fn lang_tag_is(tag: &str, names: &[&str]) -> bool {
    names.iter().any(|n| tag.eq_ignore_ascii_case(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(code: &str, lang: &str) -> Vec<(String, Color32)> {
        let theme = Theme::light();
        let job = job(code, lang, &theme, 13.0);
        job.sections.iter().map(|s| (job.text[s.byte_range.start.0..s.byte_range.end.0].to_string(), s.format.color)).collect()
    }

    #[test]
    fn python_keywords_strings_comments() {
        let p = palette(&Theme::light());
        let c = colors("def f(x):\n    return \"hi\"  # done", "python");
        assert!(c.contains(&("def".into(), p.keyword)));
        assert!(c.contains(&("f".into(), p.func)));
        assert!(c.contains(&("\"hi\"".into(), p.string)));
        assert!(c.contains(&("# done".into(), p.comment)));
    }

    #[test]
    fn shell_commands_flags_vars() {
        let p = palette(&Theme::light());
        let c = colors("xrdcp -f $SRC root://eos//x | grep ok", "bash");
        assert!(c.contains(&("xrdcp".into(), p.func)));
        assert!(c.contains(&("-f".into(), p.number)));
        assert!(c.contains(&("$SRC".into(), p.ty)));
        assert!(c.contains(&("grep".into(), p.func)));
    }
}

#[cfg(test)]
mod cpp_tests {
    use super::*;

    #[test]
    fn cpp_calls_and_types() {
        let p = palette(&Theme::light());
        let job = job("std::vector<double> fit(const TH1D& h) { return {h.GetMean(), 0.5}; }", "cpp", &Theme::light(), 13.0);
        let c: Vec<(String, Color32)> =
            job.sections.iter().map(|s| (job.text[s.byte_range.start.0..s.byte_range.end.0].to_string(), s.format.color)).collect();
        assert!(c.contains(&("fit".into(), p.func)), "{c:?}");
        assert!(c.contains(&("TH1D".into(), p.ty)));
        assert!(c.contains(&("GetMean".into(), p.func)));
    }
}
