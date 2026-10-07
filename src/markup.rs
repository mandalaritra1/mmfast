//! Mattermost markdown: parsed once per post into blocks, each paragraph
//! laid out as one egui galley (so it wraps, selects and copies as text),
//! with link, mention and channel ranges kept for hit-testing.

use crate::theme::Theme;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, Sense, Stroke, Ui};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    Url(String),
    User(String),
    Channel(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub math: bool,
    pub heading: u8,
    /// -1 subscript, 1 superscript (math).
    pub script: i8,
    /// `:name:` that is not a Unicode emoji; drawn as the server's custom
    /// emoji if it has one by that name, else left as text.
    pub custom_emoji: bool,
}

#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    pub style: Style,
    pub link: Option<Link>,
}

pub type Inline = Vec<Span>;

#[derive(Clone, Debug)]
pub enum Block {
    Para(Inline),
    Heading(u8, Inline),
    Code { lang: String, text: String },
    Math(String),
    Quote(Vec<Block>),
    List { start: Option<u64>, items: Vec<Vec<Block>> },
    Table { header: Vec<Inline>, rows: Vec<Vec<Inline>> },
    Rule,
}

pub struct Doc {
    pub blocks: Vec<Block>,
}

// ------------------------------------------------------------------ parse

pub fn parse(src: &str) -> Arc<Doc> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_MATH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(src, opts);
    let mut b = Builder::default();
    for ev in parser {
        b.event(ev);
    }
    Arc::new(Doc { blocks: b.finish() })
}

#[derive(Default)]
struct Frame {
    kind: FrameKind,
    blocks: Vec<Block>,
}

#[derive(Default)]
enum FrameKind {
    #[default]
    Root,
    Quote,
    List(Option<u64>, Vec<Vec<Block>>),
    Item,
}

/// A table being parsed: alignments, header, rows, the current row, in-head.
type TableBuild = (Vec<Alignment>, Vec<Inline>, Vec<Vec<Inline>>, Vec<Inline>, bool);

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    inline: Option<Inline>,
    heading: u8,
    style: Style,
    link: Option<String>,
    code: Option<(String, String)>,
    table: Option<TableBuild>,
}

impl Builder {
    fn top(&mut self) -> &mut Vec<Block> {
        if self.stack.is_empty() {
            self.stack.push(Frame::default());
        }
        &mut self.stack.last_mut().unwrap().blocks
    }

    fn inline(&mut self) -> &mut Inline {
        self.inline.get_or_insert_with(Vec::new)
    }

    fn flush_para(&mut self) {
        if let Some(inl) = self.inline.take()
            && !inl.iter().all(|s| s.text.trim().is_empty())
        {
            self.top().push(Block::Para(inl));
        }
    }

    fn push_text(&mut self, text: &str) {
        let style = self.style;
        if let Some(url) = self.link.clone() {
            self.inline().push(Span { text: text.to_string(), style, link: Some(Link::Url(url)) });
            return;
        }
        if style.code || style.math {
            self.inline().push(Span { text: text.to_string(), style, link: None });
            return;
        }
        let spans = scan(text, style);
        self.inline().extend(spans);
    }

    fn event(&mut self, ev: Event) {
        if let Some((_, buf)) = &mut self.code {
            match ev {
                Event::Text(t) => {
                    buf.push_str(&t);
                    return;
                }
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, text) = self.code.take().unwrap();
                    self.top().push(Block::Code { lang, text: text.trim_end_matches('\n').to_string() });
                    return;
                }
                _ => return,
            }
        }
        if let Some(table) = &mut self.table {
            match ev {
                Event::Start(Tag::TableHead) => table.4 = true,
                Event::End(TagEnd::TableHead) => {
                    table.4 = false;
                    table.1 = std::mem::take(&mut table.3);
                }
                Event::Start(Tag::TableRow) => table.3.clear(),
                Event::End(TagEnd::TableRow) => {
                    let row = std::mem::take(&mut table.3);
                    table.2.push(row);
                }
                Event::Start(Tag::TableCell) => self.inline = Some(Vec::new()),
                Event::End(TagEnd::TableCell) => {
                    let cell = self.inline.take().unwrap_or_default();
                    self.table.as_mut().unwrap().3.push(cell);
                }
                Event::End(TagEnd::Table) => {
                    let (_, header, rows, _, _) = self.table.take().unwrap();
                    self.top().push(Block::Table { header, rows });
                }
                other => self.inline_event(other),
            }
            return;
        }
        match ev {
            Event::Start(Tag::Paragraph) => self.flush_para(),
            Event::End(TagEnd::Paragraph) => self.flush_para(),
            Event::Start(Tag::Heading { level, .. }) => {
                self.flush_para();
                self.heading = level as u8;
                self.style.heading = level as u8;
            }
            Event::End(TagEnd::Heading(_)) => {
                let inl = self.inline.take().unwrap_or_default();
                let level = self.heading;
                self.style.heading = 0;
                self.top().push(Block::Heading(level, inl));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                self.flush_para();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.flush_para();
                self.stack.push(Frame { kind: FrameKind::Quote, blocks: Vec::new() });
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.flush_para();
                if let Some(f) = self.stack.pop() {
                    self.top().push(Block::Quote(f.blocks));
                }
            }
            Event::Start(Tag::List(start)) => {
                self.flush_para();
                self.stack.push(Frame { kind: FrameKind::List(start, Vec::new()), blocks: Vec::new() });
            }
            Event::End(TagEnd::List(_)) => {
                self.flush_para();
                if let Some(Frame { kind: FrameKind::List(start, items), .. }) = self.stack.pop() {
                    self.top().push(Block::List { start, items });
                }
            }
            Event::Start(Tag::Item) => {
                self.flush_para();
                self.stack.push(Frame { kind: FrameKind::Item, blocks: Vec::new() });
            }
            Event::End(TagEnd::Item) => {
                self.flush_para();
                if let Some(item) = self.stack.pop()
                    && let Some(Frame { kind: FrameKind::List(_, items), .. }) = self.stack.last_mut()
                {
                    items.push(item.blocks);
                }
            }
            Event::Start(Tag::Table(aligns)) => {
                self.flush_para();
                self.table = Some((aligns, Vec::new(), Vec::new(), Vec::new(), false));
            }
            Event::Rule => {
                self.flush_para();
                self.top().push(Block::Rule);
            }
            Event::DisplayMath(m) => {
                self.flush_para();
                self.top().push(Block::Math(m.to_string()));
            }
            other => self.inline_event(other),
        }
    }

    fn inline_event(&mut self, ev: Event) {
        match ev {
            Event::Text(t) => self.push_text(&t),
            Event::Code(t) => {
                let mut style = self.style;
                style.code = true;
                self.inline().push(Span { text: t.to_string(), style, link: None });
            }
            Event::InlineMath(m) | Event::DisplayMath(m) => {
                for (text, level) in latex_segments(&m) {
                    let mut style = self.style;
                    style.math = true;
                    style.script = level;
                    self.inline().push(Span { text, style, link: None });
                }
            }
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => self.inline().push(Span { text: "\n".into(), style: Style::default(), link: None }),
            Event::Start(Tag::Emphasis) => self.style.italic = true,
            Event::End(TagEnd::Emphasis) => self.style.italic = false,
            Event::Start(Tag::Strong) => self.style.bold = true,
            Event::End(TagEnd::Strong) => self.style.bold = false,
            Event::Start(Tag::Strikethrough) => self.style.strike = true,
            Event::End(TagEnd::Strikethrough) => self.style.strike = false,
            Event::Start(Tag::Link { dest_url, .. }) => self.link = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => self.link = None,
            Event::Start(Tag::Image { dest_url, .. }) => self.link = Some(dest_url.to_string()),
            Event::End(TagEnd::Image) => self.link = None,
            Event::TaskListMarker(done) => self.push_text(if done { "☑ " } else { "☐ " }),
            Event::Html(h) | Event::InlineHtml(h) => self.push_text(&h),
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<Block> {
        self.flush_para();
        while self.stack.len() > 1 {
            let f = self.stack.pop().unwrap();
            self.top().extend(f.blocks);
        }
        self.stack.pop().map(|f| f.blocks).unwrap_or_default()
    }
}

/// Split plain text into spans: bare URLs, @mentions, ~channels, :emoji:.
fn scan(text: &str, style: Style) -> Vec<Span> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    let at_boundary = |i: usize| i == 0 || !chars[i - 1].1.is_alphanumeric();
    let flush = |plain: &mut String, out: &mut Vec<Span>| {
        if !plain.is_empty() {
            out.push(Span { text: std::mem::take(plain), style, link: None });
        }
    };
    while i < chars.len() {
        let (pos, c) = chars[i];
        let rest = &text[pos..];
        if at_boundary(i) && (rest.starts_with("https://") || rest.starts_with("http://") || rest.starts_with("www.")) {
            let end = rest.find(|c: char| c.is_whitespace() || c == '<' || c == '>').unwrap_or(rest.len());
            let url = rest[..end].trim_end_matches(['.', ',', ')', ';', ':', '!', '?', '\'', '"']);
            flush(&mut plain, &mut out);
            let target = if url.starts_with("www.") { format!("https://{url}") } else { url.to_string() };
            out.push(Span { text: url.to_string(), style, link: Some(Link::Url(target)) });
            i += url.chars().count();
            continue;
        }
        if (c == '@' || c == '~') && at_boundary(i) {
            let name: String = rest[1..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
                .collect();
            let name = name.trim_end_matches(['.', '-', '_']).to_string();
            if !name.is_empty() {
                flush(&mut plain, &mut out);
                let link = if c == '@' { Link::User(name.clone()) } else { Link::Channel(name.clone()) };
                out.push(Span { text: format!("{c}{name}"), style, link: Some(link) });
                i += 1 + name.chars().count();
                continue;
            }
        }
        if c == ':'
            && let Some(end) = rest[1..].find(':')
        {
            let name = &rest[1..1 + end];
            if !name.is_empty()
                && name.len() < 40
                && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
            {
                if let Some(e) = emoji_for(name) {
                    plain.push_str(e);
                    i += name.chars().count() + 2;
                    continue;
                }
                if name.chars().any(|c| c.is_ascii_alphabetic()) {
                    flush(&mut plain, &mut out);
                    let mut st = style;
                    st.custom_emoji = true;
                    out.push(Span { text: format!(":{name}:"), style: st, link: None });
                    i += name.chars().count() + 2;
                    continue;
                }
            }
        }
        plain.push(c);
        i += 1;
    }
    flush(&mut plain, &mut out);
    out
}

pub fn emoji_for(name: &str) -> Option<&'static str> {
    let name = match name {
        "thumbsup" | "+1" => "+1",
        "thumbsdown" | "-1" => "-1",
        "white_check_mark" => "white_check_mark",
        other => other,
    };
    emojis::get_by_shortcode(name).map(|e| e.as_str())
}

/// A readable Unicode rendering of simple LaTeX: Greek, operators, bars,
/// super/subscripts and fractions. Anything else keeps its source.
fn latex_raw(src: &str) -> String {
    const MACROS: &[(&str, &str)] = &[
        ("alpha", "α"), ("beta", "β"), ("gamma", "γ"), ("delta", "δ"), ("epsilon", "ε"), ("varepsilon", "ε"),
        ("zeta", "ζ"), ("eta", "η"), ("theta", "θ"), ("iota", "ι"), ("kappa", "κ"), ("lambda", "λ"),
        ("mu", "μ"), ("nu", "ν"), ("xi", "ξ"), ("pi", "π"), ("rho", "ρ"), ("sigma", "σ"), ("tau", "τ"),
        ("upsilon", "υ"), ("phi", "φ"), ("varphi", "φ"), ("chi", "χ"), ("psi", "ψ"), ("omega", "ω"),
        ("Gamma", "Γ"), ("Delta", "Δ"), ("Theta", "Θ"), ("Lambda", "Λ"), ("Xi", "Ξ"), ("Pi", "Π"),
        ("Sigma", "Σ"), ("Phi", "Φ"), ("Psi", "Ψ"), ("Omega", "Ω"),
        ("to", "→"), ("rightarrow", "→"), ("leftarrow", "←"), ("Rightarrow", "⇒"), ("leftrightarrow", "↔"),
        ("pm", "±"), ("mp", "∓"), ("times", "×"), ("cdot", "·"), ("div", "÷"), ("leq", "≤"), ("le", "≤"),
        ("geq", "≥"), ("ge", "≥"), ("neq", "≠"), ("ne", "≠"), ("approx", "≈"), ("sim", "∼"), ("simeq", "≃"),
        ("equiv", "≡"), ("propto", "∝"), ("infty", "∞"), ("sum", "∑"), ("prod", "∏"), ("int", "∫"),
        ("partial", "∂"), ("nabla", "∇"), ("sqrt", "√"), ("in", "∈"), ("notin", "∉"), ("subset", "⊂"),
        ("cup", "∪"), ("cap", "∩"), ("forall", "∀"), ("exists", "∃"), ("ell", "ℓ"), ("hbar", "ℏ"),
        ("degree", "°"), ("circ", "°"), ("langle", "⟨"), ("rangle", "⟩"), ("ldots", "…"), ("cdots", "⋯"),
        ("quad", "  "), ("qquad", "    "), (",", " "), (";", " "), ("!", ""), (" ", " "),
        ("log", "log"), ("ln", "ln"), ("exp", "exp"), ("sin", "sin"), ("cos", "cos"), ("max", "max"),
        ("min", "min"), ("lim", "lim"),
    ];
    const SUP: &[(char, char)] = &[
        ('0', '⁰'), ('1', '¹'), ('2', '²'), ('3', '³'), ('4', '⁴'), ('5', '⁵'), ('6', '⁶'), ('7', '⁷'),
        ('8', '⁸'), ('9', '⁹'), ('+', '⁺'), ('-', '⁻'), ('=', '⁼'), ('(', '⁽'), (')', '⁾'), ('n', 'ⁿ'),
        ('i', 'ⁱ'), ('*', '*'),
    ];
    const SUB: &[(char, char)] = &[
        ('0', '₀'), ('1', '₁'), ('2', '₂'), ('3', '₃'), ('4', '₄'), ('5', '₅'), ('6', '₆'), ('7', '₇'),
        ('8', '₈'), ('9', '₉'), ('+', '₊'), ('-', '₋'), ('=', '₌'), ('(', '₍'), (')', '₎'), ('a', 'ₐ'),
        ('e', 'ₑ'), ('o', 'ₒ'), ('x', 'ₓ'), ('h', 'ₕ'), ('k', 'ₖ'), ('l', 'ₗ'), ('m', 'ₘ'), ('n', 'ₙ'),
        ('p', 'ₚ'), ('s', 'ₛ'), ('t', 'ₜ'), ('i', 'ᵢ'), ('j', 'ⱼ'), ('r', 'ᵣ'), ('u', 'ᵤ'), ('v', 'ᵥ'),
    ];

    fn group(chars: &[char], i: &mut usize) -> String {
        // A {...} group or a single char / macro starting at i.
        if *i >= chars.len() {
            return String::new();
        }
        if chars[*i] == '{' {
            let mut depth = 0;
            let start = *i + 1;
            while *i < chars.len() {
                match chars[*i] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            let inner: String = chars[start..*i].iter().collect();
                            *i += 1;
                            return inner;
                        }
                    }
                    _ => {}
                }
                *i += 1;
            }
            chars[start..].iter().collect()
        } else if chars[*i] == '\\' {
            let start = *i;
            *i += 1;
            while *i < chars.len() && chars[*i].is_ascii_alphabetic() {
                *i += 1;
            }
            chars[start..*i].iter().collect()
        } else {
            *i += 1;
            chars[*i - 1].to_string()
        }
    }

    fn convert(src: &str) -> String {
        // Nesting and size budgets: past them the source is shown as written.
        thread_local!(static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
        let depth = DEPTH.with(|d| d.get());
        if depth > 24 || src.len() > 4000 {
            return src.to_string();
        }
        DEPTH.with(|d| d.set(depth + 1));
        let out = convert_inner(src);
        DEPTH.with(|d| d.set(depth));
        out
    }

    fn convert_inner(src: &str) -> String {
        let chars: Vec<char> = src.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            match c {
                '\\' => {
                    let mut j = i + 1;
                    while j < chars.len() && chars[j].is_ascii_alphabetic() {
                        j += 1;
                    }
                    if j == i + 1 && j < chars.len() {
                        j += 1;
                    }
                    let name: String = chars[i + 1..j].iter().collect();
                    i = j;
                    match name.as_str() {
                        "frac" | "dfrac" | "tfrac" => {
                            let a = convert(&group(&chars, &mut i));
                            let b = convert(&group(&chars, &mut i));
                            let wrap = |s: &str| if s.chars().count() > 1 { format!("({s})") } else { s.to_string() };
                            out.push_str(&format!("{}/{}", wrap(&a), wrap(&b)));
                        }
                        "bar" | "overline" => {
                            let a: Vec<char> = convert(&group(&chars, &mut i)).chars().collect();
                            for (k, &ch) in a.iter().enumerate() {
                                out.push(ch);
                                // One bar per base letter: a bar over a bar must
                                // not grow the text (it once doubled per level).
                                if !is_combining(ch) && !is_marker(ch) && a.get(k + 1) != Some(&'\u{0304}') {
                                    out.push('\u{0304}');
                                }
                            }
                        }
                        "hat" => {
                            out.push_str(&convert(&group(&chars, &mut i)));
                            out.push('\u{0302}');
                        }
                        "vec" => {
                            out.push_str(&convert(&group(&chars, &mut i)));
                            out.push('\u{20D7}');
                        }
                        "tilde" => {
                            out.push_str(&convert(&group(&chars, &mut i)));
                            out.push('\u{0303}');
                        }
                        "mathrm" | "text" | "mathbf" | "mathit" | "operatorname" | "textrm" | "mathcal" | "boldsymbol" => {
                            out.push_str(&convert(&group(&chars, &mut i)));
                        }
                        "left" | "right" | "big" | "Big" | "displaystyle" => {}
                        _ => match MACROS.iter().find(|(m, _)| *m == name) {
                            Some((_, u)) if u.chars().count() == 1 => out.push(crate::math::italic(u.chars().next().unwrap())),
                            Some((_, u)) => out.push_str(u),
                            None => {
                                out.push('\\');
                                out.push_str(&name);
                            }
                        },
                    }
                }
                '^' | '_' => {
                    i += 1;
                    let g = convert(&group(&chars, &mut i));
                    let table = if c == '^' { SUP } else { SUB };
                    let mapped: Option<String> =
                        g.chars().map(|ch| table.iter().find(|(a, _)| *a == ch).map(|(_, b)| *b)).collect();
                    match mapped {
                        Some(m) => out.push_str(&m),
                        None => {
                            // Marked for real sub/superscript layout.
                            let g: String = g.chars().filter(|ch| !is_marker(*ch)).collect();
                            out.push(if c == '^' { SUP_START } else { SUB_START });
                            out.push_str(&g);
                            out.push(SCRIPT_END);
                        }
                    }
                }
                '{' | '}' => i += 1,
                _ => {
                    // Variables in math italic, as TeX sets them.
                    out.push(crate::math::italic(c));
                    i += 1;
                }
            }
        }
        out
    }

    convert(src.trim())
}

const SUB_START: char = '\u{E000}';
const SUP_START: char = '\u{E001}';
const SCRIPT_END: char = '\u{E002}';

fn is_combining(c: char) -> bool {
    matches!(c, '\u{0300}'..='\u{036F}' | '\u{20D0}'..='\u{20FF}')
}

fn is_marker(c: char) -> bool {
    matches!(c, SUB_START | SUP_START | SCRIPT_END)
}

/// LaTeX as runs of (text, level): 0 baseline, -1 subscript, 1 superscript.
pub fn latex_segments(src: &str) -> Vec<(String, i8)> {
    let mut out: Vec<(String, i8)> = vec![(String::new(), 0)];
    for c in latex_raw(src).chars() {
        match c {
            SUB_START => out.push((String::new(), -1)),
            SUP_START => out.push((String::new(), 1)),
            SCRIPT_END => out.push((String::new(), 0)),
            c => out.last_mut().unwrap().0.push(c),
        }
    }
    out.retain(|(s, _)| !s.is_empty());
    out
}

/// Plain-text rendering (previews, tooltips).
pub fn latex_to_unicode(src: &str) -> String {
    let mut s = String::new();
    for (text, level) in latex_segments(src) {
        match level {
            0 => s.push_str(&text),
            l => {
                let c = if l > 0 { '^' } else { '_' };
                if text.chars().count() == 1 {
                    s.push(c);
                    s.push_str(&text);
                } else {
                    s.push_str(&format!("{c}({text})"));
                }
            }
        }
    }
    s
}

// ------------------------------------------------------------------ render

pub struct RenderCtx<'a> {
    pub theme: &'a Theme,
    pub size: f32,
    pub me: &'a str,
    pub color: Color32,
    /// Search terms to mark in plain text.
    pub highlight: &'a [String],
    /// The server's custom emoji, name to id.
    pub emoji: &'a std::collections::HashMap<String, String>,
    /// Where custom emoji landed (screen rect, emoji id), for the caller to paint.
    pub paint: Option<&'a std::cell::RefCell<Vec<(egui::Rect, String)>>>,
}

fn font(style: Style, size: f32) -> FontId {
    let size = match style.heading {
        1 => size * 1.45,
        2 => size * 1.3,
        3 => size * 1.15,
        0 => size,
        _ => size * 1.05,
    };
    let size = if style.script != 0 { size * 0.72 } else { size };
    if style.math {
        FontId::new(size * 1.05, crate::math::family())
    } else if style.code {
        FontId::new(size * 0.92, FontFamily::Monospace)
    } else if style.bold || style.heading > 0 {
        fastframe_fonts::Weight::SemiBold.font_id(size)
    } else {
        FontId::proportional(size)
    }
}

/// Lay out spans as one job; returns the link ranges in char indices.
pub fn layout(inl: &[Span], rc: &RenderCtx, width: f32) -> (LayoutJob, Vec<(Range<usize>, Link)>) {
    layout_with_emoji(inl, rc, width, &mut Vec::new())
}

/// Like [`layout`], also returning where custom emoji placeholders sit (char index, id).
pub fn layout_with_emoji(
    inl: &[Span],
    rc: &RenderCtx,
    width: f32,
    emoji_at: &mut Vec<(usize, String)>,
) -> (LayoutJob, Vec<(Range<usize>, Link)>) {
    let mut job = LayoutJob { wrap: egui::text::TextWrapping { max_width: width, ..Default::default() }, ..Default::default() };
    let mut links = Vec::new();
    let mut chars = 0usize;
    for span in inl {
        if span.style.custom_emoji
            && let Some(id) = rc.emoji.get(span.text.trim_matches(':'))
        {
            // An ideographic space is about one em wide: room for the picture.
            emoji_at.push((chars, id.clone()));
            let fmt = TextFormat { font_id: font(span.style, rc.size * 1.15), color: Color32::TRANSPARENT, ..Default::default() };
            job.append("\u{3000}", 0.0, fmt);
            chars += 1;
            continue;
        }
        let mut fmt = TextFormat { font_id: font(span.style, rc.size), color: rc.color, ..Default::default() };
        fmt.italics = span.style.italic;
        match span.style.script {
            -1 => fmt.valign = egui::Align::BOTTOM,
            1 => fmt.valign = egui::Align::TOP,
            _ => {}
        }
        if span.style.strike {
            fmt.strikethrough = Stroke::new(1.0, rc.color);
        }
        if span.style.code {
            fmt.background = rc.theme.code_bg;
            fmt.color = rc.theme.code_text;
        }
        if span.style.math {
            fmt.color = rc.theme.math;
        }
        if let Some(link) = &span.link {
            match link {
                Link::Url(_) => {
                    fmt.color = rc.theme.link;
                }
                Link::User(name) => {
                    fmt.color = rc.theme.link;
                    if name == rc.me || name == "here" || name == "channel" || name == "all" {
                        fmt.background = rc.theme.mention_bg;
                        fmt.color = rc.theme.mention_text;
                    }
                }
                Link::Channel(_) => fmt.color = rc.theme.link,
            }
            let n = span.text.chars().count();
            links.push((chars..chars + n, link.clone()));
        }
        chars += span.text.chars().count();
        if span.link.is_none() && !span.style.code && !rc.highlight.is_empty() {
            append_marked(&mut job, &span.text, fmt, rc);
        } else {
            job.append(&span.text, 0.0, fmt);
        }
    }
    (job, links)
}

/// Append text with search terms on the mention background.
fn append_marked(job: &mut LayoutJob, text: &str, fmt: TextFormat, rc: &RenderCtx) {
    let lower = text.to_lowercase();
    if lower.len() != text.len() {
        job.append(text, 0.0, fmt);
        return;
    }
    let mut marks: Vec<(usize, usize)> = Vec::new();
    for term in rc.highlight {
        let term = term.to_lowercase();
        if term.len() < 2 {
            continue;
        }
        let mut from = 0;
        while let Some(i) = lower[from..].find(&term) {
            let start = from + i;
            marks.push((start, start + term.len()));
            from = start + term.len();
        }
    }
    marks.sort();
    let mut at = 0;
    for (start, end) in marks {
        if start < at || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        job.append(&text[at..start], 0.0, fmt.clone());
        let mut hit = fmt.clone();
        hit.background = rc.theme.mention_bg;
        hit.color = rc.theme.mention_text;
        job.append(&text[start..end], 0.0, hit);
        at = end;
    }
    job.append(&text[at..], 0.0, fmt);
}

/// Draw one laid-out paragraph; returns a link if one was clicked.
pub fn paragraph(ui: &mut Ui, inl: &[Span], rc: &RenderCtx) -> Option<Link> {
    let width = ui.available_width();
    let mut emoji_at = Vec::new();
    let (job, links) = layout_with_emoji(inl, rc, width, &mut emoji_at);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let resp = ui.add(egui::Label::new(galley.clone()).selectable(true).sense(Sense::click()));
    if let Some(paint) = rc.paint {
        for (idx, id) in emoji_at {
            let r = galley.pos_from_cursor(egui::text::CCursor::new(idx));
            let next = galley.pos_from_cursor(egui::text::CCursor::new(idx + 1));
            let room = if (next.min.y - r.min.y).abs() < 1.0 { next.min.x - r.min.x } else { rc.size };
            let h = r.height().min(rc.size * 1.35).min(room.max(8.0));
            let rect = egui::Rect::from_min_size(resp.rect.min + r.min.to_vec2() + egui::vec2(1.0, (r.height() - h) / 2.0), egui::vec2(h, h));
            paint.borrow_mut().push((rect, id));
        }
    }
    if links.is_empty() {
        return None;
    }
    let pos = resp.hover_pos().or_else(|| resp.interact_pointer_pos())?;
    let cursor = galley.cursor_from_pos(pos - resp.rect.min);
    let hit = links.iter().find(|(r, _)| r.contains(&cursor.index.0))?;
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if let Link::Url(u) = &hit.1 {
            resp.clone().on_hover_text_at_pointer(u);
        }
    }
    if resp.clicked() { Some(hit.1.clone()) } else { None }
}

pub fn show(ui: &mut Ui, doc: &Doc, rc: &RenderCtx) -> Option<Link> {
    let mut clicked = None;
    let spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing.y = 4.0;
    for block in &doc.blocks {
        if let Some(l) = show_block(ui, block, rc) {
            clicked = Some(l);
        }
    }
    ui.spacing_mut().item_spacing = spacing;
    clicked
}

fn show_block(ui: &mut Ui, block: &Block, rc: &RenderCtx) -> Option<Link> {
    // Quotes in lists in quotes…: past a sane depth, draw the rest as text.
    thread_local!(static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
    let depth = DEPTH.with(|d| d.get());
    if depth > 12 {
        let text = plain_blocks(std::slice::from_ref(block));
        ui.label(egui::RichText::new(text).color(rc.theme.text_dim));
        return None;
    }
    DEPTH.with(|d| d.set(depth + 1));
    let out = show_block_inner(ui, block, rc);
    DEPTH.with(|d| d.set(depth));
    out
}

fn show_block_inner(ui: &mut Ui, block: &Block, rc: &RenderCtx) -> Option<Link> {
    let t = rc.theme;
    match block {
        Block::Para(inl) => paragraph(ui, inl, rc),
        Block::Heading(level, inl) => {
            let spans: Vec<Span> = inl
                .iter()
                .cloned()
                .map(|mut s| {
                    s.style.heading = *level;
                    s
                })
                .collect();
            paragraph(ui, &spans, rc)
        }
        Block::Code { text, lang } if matches!(lang.as_str(), "latex" | "tex") => {
            display_math(ui, text, rc);
            None
        }
        Block::Code { text, lang } => {
            egui::Frame::new()
                .fill(t.code_block_bg)
                .stroke(Stroke::new(1.0, t.border))
                .corner_radius(4)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut copy = false;
                    ui.horizontal(|ui| {
                        if !lang.is_empty() {
                            ui.label(egui::RichText::new(lang).size(rc.size * 0.75).color(t.text_dim));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            copy = ui.small_button("Copy").clicked();
                        });
                    });
                    if copy {
                        ui.ctx().copy_text(text.clone());
                    }
                    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                    egui::ScrollArea::horizontal()
                        .id_salt(text.len() ^ (text.as_ptr() as usize))
                        // A visible bar says a cut-off line scrolls.
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                        .show(ui, |ui| {
                        let job = crate::highlight::job(text, lang, t, rc.size * 0.9);
                        let galley = ui.fonts_mut(|f| f.layout_job(job));
                        ui.add(egui::Label::new(galley).selectable(true).extend());
                    });
                });
            None
        }
        Block::Math(src) => {
            display_math(ui, src, rc);
            None
        }
        Block::Quote(blocks) => {
            let mut clicked = None;
            let resp = egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 0, top: 2, bottom: 2 }).show(ui, |ui| {
                let quote = RenderCtx { color: t.text_dim, ..*rc };
                for b in blocks {
                    if let Some(l) = show_block(ui, b, &quote) {
                        clicked = Some(l);
                    }
                }
            });
            let r = resp.response.rect;
            ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(3.0, r.height())), 1.0, t.border_strong);
            clicked
        }
        Block::List { start, items } => {
            let mut clicked = None;
            for (n, item) in items.iter().enumerate() {
                ui.horizontal_top(|ui| {
                    let marker = match start {
                        Some(s) => format!("{}.", s + n as u64),
                        None => "•".to_string(),
                    };
                    ui.add_sized(
                        egui::vec2(18.0, rc.size * 1.3),
                        egui::Label::new(egui::RichText::new(marker).size(rc.size).color(t.text_dim)),
                    );
                    ui.vertical(|ui| {
                        for b in item {
                            if let Some(l) = show_block(ui, b, rc) {
                                clicked = Some(l);
                            }
                        }
                    });
                });
            }
            clicked
        }
        Block::Table { header, rows } => {
            let mut clicked = None;
            egui::ScrollArea::horizontal().id_salt(ui.next_auto_id()).show(ui, |ui| {
                egui::Frame::new().stroke(Stroke::new(1.0, t.border)).corner_radius(4).show(ui, |ui| {
                    egui::Grid::new(ui.next_auto_id()).striped(true).spacing([18.0, 6.0]).min_col_width(40.0).show(
                        ui,
                        |ui| {
                            let bold = RenderCtx { ..*rc };
                            for cell in header {
                                let spans: Vec<Span> = cell
                                    .iter()
                                    .cloned()
                                    .map(|mut s| {
                                        s.style.bold = true;
                                        s
                                    })
                                    .collect();
                                if let Some(l) = cell_label(ui, &spans, &bold) {
                                    clicked = Some(l);
                                }
                            }
                            ui.end_row();
                            for row in rows {
                                for cell in row {
                                        if let Some(l) = cell_label(ui, cell, rc) {
                                        clicked = Some(l);
                                    }
                                }
                                ui.end_row();
                            }
                        },
                    );
                });
            });
            clicked
        }
        Block::Rule => {
            ui.separator();
            None
        }
    }
}

/// Display math, laid out by the math engine; scrolls sideways when wide.
/// Hover shows the source; a right-click copies it.
fn display_math(ui: &mut Ui, src: &str, rc: &RenderCtx) {
    let t = rc.theme;
    let size = rc.size * 1.25;
    egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
        let need = crate::math::width(ui, src, size);
        let avail = ui.available_width();
        let resp = if need + 8.0 > avail {
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            egui::ScrollArea::horizontal()
                .id_salt(("math", src))
                .show(ui, |ui| {
                    ui.set_min_width(need + 8.0);
                    crate::math::show(ui, src, size, t.text)
                })
                .inner
        } else {
            crate::math::show(ui, src, size, t.text)
        };
        let resp = resp.interact(Sense::click()).on_hover_text(src);
        resp.context_menu(|ui| {
            if ui.button("Copy LaTeX").clicked() {
                ui.ctx().copy_text(src.to_string());
                ui.close();
            }
        });
    });
}

fn cell_label(ui: &mut Ui, inl: &[Span], rc: &RenderCtx) -> Option<Link> {
    let (mut job, _) = layout(inl, rc, f32::INFINITY);
    job.wrap.max_width = f32::INFINITY;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.add(egui::Label::new(galley).selectable(true));
    None
}

/// Plain one-line preview of a message (sidebar, search, threads list).
pub fn plain(src: &str) -> String {
    plain_blocks(&parse(src).blocks)
}

fn plain_blocks(blocks: &[Block]) -> String {
    let mut out = String::new();
    fn walk(blocks: &[Block], out: &mut String) {
        for b in blocks {
            match b {
                Block::Para(inl) | Block::Heading(_, inl) => {
                    for s in inl {
                        out.push_str(&s.text);
                    }
                    out.push(' ');
                }
                Block::Code { text, .. } => {
                    out.push_str(text.lines().next().unwrap_or_default());
                    out.push(' ');
                }
                Block::Math(m) => out.push_str(&latex_to_unicode(m)),
                Block::Quote(b) => walk(b, out),
                Block::List { items, .. } => {
                    for i in items {
                        walk(i, out);
                    }
                }
                Block::Table { header, .. } => {
                    for c in header {
                        for s in c {
                            out.push_str(&s.text);
                        }
                        out.push(' ');
                    }
                }
                Block::Rule => {}
            }
        }
    }
    walk(blocks, &mut out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latex_bar_and_frac() {
        assert_eq!(latex_to_unicode("m_{t\\bar{t}}"), "𝑚_(𝑡𝑡̄)");
        assert_eq!(latex_to_unicode("x^2"), "𝑥²");
        assert_eq!(latex_to_unicode("\\frac{a}{b}"), "𝑎/𝑏");
        assert_eq!(latex_to_unicode("\\rho = \\log(m^2/p_T^2)"), "𝜌 = log(𝑚²/𝑝_𝑇²)");
        assert_eq!(latex_segments("m_{t\\bar{t}}"), vec![("𝑚".to_string(), 0), ("𝑡𝑡\u{304}".to_string(), -1)]);
    }

    #[test]
    fn nested_bars_do_not_explode() {
        let mut src = "x".to_string();
        for _ in 0..40 {
            src = format!("\\bar{{{src}}}");
        }
        let out = latex_to_unicode(&src);
        // Linear in the input (it was 2^40 characters before).
        assert!(out.chars().count() < 2 * src.chars().count(), "{}", out.chars().count());
        let deep = "{".repeat(5000) + "x" + &"}".repeat(5000);
        let _ = latex_to_unicode(&deep);
        let _ = parse(&format!("{}x", "> ".repeat(500)));
    }

    #[test]
    fn mentions_and_emoji() {
        let spans = scan("hi @alice see ~town-square :tada: https://x.org/a.", Style::default());
        let links: Vec<_> = spans.iter().filter_map(|s| s.link.clone()).collect();
        assert_eq!(
            links,
            vec![Link::User("alice".into()), Link::Channel("town-square".into()), Link::Url("https://x.org/a".into())]
        );
        assert!(spans.iter().any(|s| s.text.contains('🎉')));
    }
}
