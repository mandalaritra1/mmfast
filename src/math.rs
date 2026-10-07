//! A small TeX-style math layout engine for display math: fractions, roots,
//! scripts, big operators with limits, accents, `\left…\right`, matrices and
//! cases. It lays out boxes (width, ascent, descent) and paints them with
//! the system's STIX Two Math font, so `$$…$$` reads like a paper rather
//! than like code. Inline `$…$` stays text (see `markup.rs`).

use egui::epaint::text::{FontsView, Galley};
use egui::{Color32, FontFamily, FontId, Pos2, Rect, Stroke, Ui, Vec2, pos2, vec2};
use std::sync::Arc;

/// The egui family the math font is installed under (see `ui::install_fonts`).
pub const FAMILY: &str = "math";

pub fn family() -> FontFamily {
    FontFamily::Name(FAMILY.into())
}

/// Where the math font lives on this system, if anywhere.
pub fn font_path() -> Option<&'static str> {
    ["/System/Library/Fonts/Supplemental/STIXTwoMath.otf", "/usr/share/fonts/opentype/stix/STIXTwoMath-Regular.otf"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
}

// ------------------------------------------------------------------ tree

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Ord,
    /// sum, product, integral, lim: may carry limits above and below
    Large,
    /// sin, log, max: upright, thin space after
    Func,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
}

#[derive(Clone, Copy, Debug)]
enum Accent {
    Bar,
    Hat,
    Vec,
    Tilde,
    Dot,
    Ddot,
}

#[derive(Clone, Debug)]
enum Node {
    Sym(String, Kind),
    Row(Vec<Node>),
    Frac(Box<Node>, Box<Node>),
    Sqrt(Option<Box<Node>>, Box<Node>),
    Scripts { base: Box<Node>, sub: Option<Box<Node>>, sup: Option<Box<Node>> },
    Accent(Box<Node>, Accent),
    Text(String),
    Space(f32),
    Delim { left: String, body: Box<Node>, right: String },
    Matrix { rows: Vec<Vec<Node>>, aligned: bool, cases: bool },
}

impl Node {
    fn kind(&self) -> Kind {
        match self {
            Node::Sym(_, k) => *k,
            Node::Scripts { base, .. } => base.kind(),
            _ => Kind::Ord,
        }
    }
}

/// Mathematical italic for Latin and lowercase Greek letters, as TeX sets variables.
pub fn italic(c: char) -> char {
    let code = match c {
        'h' => 0x210E,
        'a'..='z' => 0x1D44E + (c as u32 - 'a' as u32),
        'A'..='Z' => 0x1D434 + (c as u32 - 'A' as u32),
        'α'..='ω' => 0x1D6FC + (c as u32 - 'α' as u32),
        _ => return c,
    };
    char::from_u32(code).unwrap_or(c)
}

fn symbol(name: &str) -> Option<(&'static str, Kind)> {
    use Kind::*;
    Some(match name {
        "alpha" => ("α", Ord), "beta" => ("β", Ord), "gamma" => ("γ", Ord), "delta" => ("δ", Ord),
        "epsilon" => ("ϵ", Ord), "varepsilon" => ("ε", Ord), "zeta" => ("ζ", Ord), "eta" => ("η", Ord),
        "theta" => ("θ", Ord), "vartheta" => ("ϑ", Ord), "iota" => ("ι", Ord), "kappa" => ("κ", Ord),
        "lambda" => ("λ", Ord), "mu" => ("μ", Ord), "nu" => ("ν", Ord), "xi" => ("ξ", Ord), "pi" => ("π", Ord),
        "rho" => ("ρ", Ord), "sigma" => ("σ", Ord), "tau" => ("τ", Ord), "upsilon" => ("υ", Ord),
        "phi" => ("ϕ", Ord), "varphi" => ("φ", Ord), "chi" => ("χ", Ord), "psi" => ("ψ", Ord), "omega" => ("ω", Ord),
        "Gamma" => ("Γ", Ord), "Delta" => ("Δ", Ord), "Theta" => ("Θ", Ord), "Lambda" => ("Λ", Ord),
        "Xi" => ("Ξ", Ord), "Pi" => ("Π", Ord), "Sigma" => ("Σ", Ord), "Phi" => ("Φ", Ord), "Psi" => ("Ψ", Ord),
        "Omega" => ("Ω", Ord), "infty" => ("∞", Ord), "partial" => ("∂", Ord), "nabla" => ("∇", Ord),
        "hbar" => ("ℏ", Ord), "ell" => ("ℓ", Ord), "dagger" => ("†", Ord), "prime" => ("′", Ord),
        "ldots" | "dots" => ("…", Ord), "cdots" => ("⋯", Ord), "degree" => ("°", Ord), "emptyset" => ("∅", Ord),
        "forall" => ("∀", Ord), "exists" => ("∃", Ord), "neg" => ("¬", Ord), "angle" => ("∠", Ord),
        "pm" => ("±", Bin), "mp" => ("∓", Bin), "times" => ("×", Bin), "cdot" => ("⋅", Bin), "div" => ("÷", Bin),
        "otimes" => ("⊗", Bin), "oplus" => ("⊕", Bin), "cup" => ("∪", Bin), "cap" => ("∩", Bin),
        "circ" => ("∘", Bin), "ast" => ("∗", Bin), "wedge" => ("∧", Bin), "vee" => ("∨", Bin),
        "leq" | "le" => ("≤", Rel), "geq" | "ge" => ("≥", Rel), "neq" | "ne" => ("≠", Rel),
        "approx" => ("≈", Rel), "sim" => ("∼", Rel), "simeq" => ("≃", Rel), "equiv" => ("≡", Rel),
        "propto" => ("∝", Rel), "ll" => ("≪", Rel), "gg" => ("≫", Rel), "in" => ("∈", Rel),
        "notin" => ("∉", Rel), "subset" => ("⊂", Rel), "subseteq" => ("⊆", Rel), "supset" => ("⊃", Rel),
        "to" | "rightarrow" => ("→", Rel), "leftarrow" | "gets" => ("←", Rel), "Rightarrow" => ("⇒", Rel),
        "Leftarrow" => ("⇐", Rel), "leftrightarrow" => ("↔", Rel), "Leftrightarrow" | "iff" => ("⇔", Rel),
        "mapsto" => ("↦", Rel), "perp" => ("⊥", Rel), "parallel" => ("∥", Rel),
        "sum" => ("∑", Large), "prod" => ("∏", Large), "int" => ("∫", Large), "oint" => ("∮", Large),
        "iint" => ("∬", Large), "iiint" => ("∭", Large), "bigcup" => ("⋃", Large), "bigcap" => ("⋂", Large),
        "langle" => ("⟨", Open), "rangle" => ("⟩", Close), "lbrace" | "{" => ("{", Open), "rbrace" | "}" => ("}", Close),
        "lvert" | "rvert" | "vert" | "|" => ("|", Ord), "lVert" | "rVert" | "Vert" => ("‖", Ord),
        "lfloor" => ("⌊", Open), "rfloor" => ("⌋", Close), "lceil" => ("⌈", Open), "rceil" => ("⌉", Close),
        "%" => ("%", Ord), "$" => ("$", Ord), "#" => ("#", Ord), "&" => ("&", Ord), "_" => ("_", Ord),
        _ => return None,
    })
}

const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "log", "ln",
    "lg", "exp", "det", "dim", "arg", "deg", "gcd", "hom", "ker", "Pr", "mod", "max", "min", "lim", "sup", "inf",
    "limsup", "liminf",
];

fn takes_limits(name: &str) -> bool {
    matches!(name, "∑" | "∏" | "⋃" | "⋂" | "lim" | "max" | "min" | "sup" | "inf" | "limsup" | "liminf" | "det")
}

// ------------------------------------------------------------------ parse

struct Parser {
    c: Vec<char>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    fn starts(&self, s: &str) -> bool {
        let n = s.chars().count();
        self.c.get(self.i..self.i + n).is_some_and(|w| w.iter().copied().eq(s.chars()))
    }

    fn command_name(&mut self) -> String {
        // After the backslash.
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        if self.i == start && self.peek().is_some() {
            self.i += 1;
        }
        self.c[start..self.i].iter().collect()
    }

    fn group_text(&mut self) -> String {
        self.skip_ws();
        if self.peek() != Some('{') {
            return self.peek().map(|c| {
                self.i += 1;
                c.to_string()
            }).unwrap_or_default();
        }
        self.i += 1;
        let start = self.i;
        let mut depth = 1;
        while let Some(c) = self.peek() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let s: String = self.c[start..self.i].iter().collect();
                        self.i += 1;
                        return s;
                    }
                }
                _ => {}
            }
            self.i += 1;
        }
        self.c[start..].iter().collect()
    }

    /// One argument: a {group}, a command, or a single character.
    fn argument(&mut self) -> Node {
        self.skip_ws();
        match self.peek() {
            Some('{') => {
                self.i += 1;
                let row = self.row(&["}"]);
                if self.peek() == Some('}') {
                    self.i += 1;
                }
                Node::Row(row)
            }
            Some(_) => self.atom().unwrap_or(Node::Row(Vec::new())),
            None => Node::Row(Vec::new()),
        }
    }

    fn delimiter(&mut self) -> String {
        self.skip_ws();
        match self.peek() {
            Some('\\') => {
                self.i += 1;
                let name = self.command_name();
                symbol(&name).map(|(s, _)| s.to_string()).unwrap_or_default()
            }
            Some('.') => {
                self.i += 1;
                String::new()
            }
            Some(c) => {
                self.i += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }

    fn row(&mut self, stop: &[&str]) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() || stop.iter().any(|s| self.starts(s)) {
                return out;
            }
            match self.peek() {
                Some('^') | Some('_') | Some('\'') => {
                    let base = out.pop().unwrap_or(Node::Row(Vec::new()));
                    let (mut base, mut sub, mut sup) = match base {
                        Node::Scripts { base, sub, sup } => (*base, sub, sup),
                        other => (other, None, None),
                    };
                    let c = self.peek().unwrap();
                    self.i += 1;
                    match c {
                        '\'' => {
                            let prime = Node::Sym("′".into(), Kind::Ord);
                            sup = Some(Box::new(match sup {
                                Some(s) => Node::Row(vec![*s, prime]),
                                None => prime,
                            }));
                        }
                        '^' => sup = Some(Box::new(self.argument())),
                        _ => sub = Some(Box::new(self.argument())),
                    }
                    if let Node::Row(r) = &base
                        && r.is_empty()
                    {
                        base = Node::Sym(String::new(), Kind::Ord);
                    }
                    out.push(Node::Scripts { base: Box::new(base), sub, sup });
                }
                _ => {
                    if let Some(n) = self.atom() {
                        out.push(n);
                    }
                }
            }
        }
    }

    fn atom(&mut self) -> Option<Node> {
        let c = self.peek()?;
        self.i += 1;
        Some(match c {
            '{' => {
                let row = self.row(&["}"]);
                if self.peek() == Some('}') {
                    self.i += 1;
                }
                Node::Row(row)
            }
            '\\' => return self.command(),
            '0'..='9' | '.' => {
                let mut s = c.to_string();
                while let Some(d) = self.peek().filter(|d| d.is_ascii_digit() || *d == '.') {
                    s.push(d);
                    self.i += 1;
                }
                Node::Sym(s, Kind::Ord)
            }
            '+' => Node::Sym("+".into(), Kind::Bin),
            '-' => Node::Sym("−".into(), Kind::Bin),
            '*' => Node::Sym("∗".into(), Kind::Bin),
            '=' | '<' | '>' => Node::Sym(c.to_string(), Kind::Rel),
            '(' | '[' => Node::Sym(c.to_string(), Kind::Open),
            ')' | ']' => Node::Sym(c.to_string(), Kind::Close),
            ',' | ';' => Node::Sym(c.to_string(), Kind::Punct),
            '&' | '}' => return None,
            c if c.is_alphabetic() => Node::Sym(italic(c).to_string(), Kind::Ord),
            c => Node::Sym(c.to_string(), Kind::Ord),
        })
    }

    fn command(&mut self) -> Option<Node> {
        let name = self.command_name();
        let em = |f: f32| Some(Node::Space(f));
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let a = self.argument();
                let b = self.argument();
                Some(Node::Frac(Box::new(a), Box::new(b)))
            }
            "binom" => {
                let a = self.argument();
                let b = self.argument();
                Some(Node::Delim {
                    left: "(".into(),
                    body: Box::new(Node::Matrix { rows: vec![vec![a], vec![b]], aligned: false, cases: false }),
                    right: ")".into(),
                })
            }
            "sqrt" => {
                self.skip_ws();
                let index = if self.peek() == Some('[') {
                    self.i += 1;
                    let r = self.row(&["]"]);
                    if self.peek() == Some(']') {
                        self.i += 1;
                    }
                    Some(Box::new(Node::Row(r)))
                } else {
                    None
                };
                Some(Node::Sqrt(index, Box::new(self.argument())))
            }
            "bar" | "overline" => Some(Node::Accent(Box::new(self.argument()), Accent::Bar)),
            "hat" | "widehat" => Some(Node::Accent(Box::new(self.argument()), Accent::Hat)),
            "vec" | "overrightarrow" => Some(Node::Accent(Box::new(self.argument()), Accent::Vec)),
            "tilde" | "widetilde" => Some(Node::Accent(Box::new(self.argument()), Accent::Tilde)),
            "dot" => Some(Node::Accent(Box::new(self.argument()), Accent::Dot)),
            "ddot" => Some(Node::Accent(Box::new(self.argument()), Accent::Ddot)),
            "text" | "textrm" | "mathrm" | "operatorname" | "mathbf" | "textbf" | "mathsf" | "texttt" | "rm" => {
                Some(Node::Text(self.group_text()))
            }
            "mathit" | "mathcal" | "boldsymbol" | "mathbb" | "pmb" => Some(self.argument()),
            "left" => {
                let left = self.delimiter();
                let body = self.row(&["\\right"]);
                let right = if self.starts("\\right") {
                    self.i += 6;
                    self.delimiter()
                } else {
                    String::new()
                };
                Some(Node::Delim { left, body: Box::new(Node::Row(body)), right })
            }
            "right" => None,
            "begin" => {
                let env = self.group_text();
                if env == "array" {
                    self.group_text();
                }
                let mut rows = Vec::new();
                let mut row = Vec::new();
                loop {
                    let cell = self.row(&["&", "\\\\", "\\end"]);
                    row.push(Node::Row(cell));
                    if self.starts("&") {
                        self.i += 1;
                    } else if self.starts("\\\\") {
                        self.i += 2;
                        rows.push(std::mem::take(&mut row));
                    } else {
                        if self.starts("\\end") {
                            self.i += 4;
                            self.group_text();
                        }
                        break;
                    }
                }
                if !(row.len() == 1 && matches!(&row[0], Node::Row(r) if r.is_empty())) {
                    rows.push(row);
                }
                let (l, r, aligned) = match env.trim_end_matches('*') {
                    "pmatrix" => ("(", ")", false),
                    "bmatrix" => ("[", "]", false),
                    "Bmatrix" => ("{", "}", false),
                    "vmatrix" => ("|", "|", false),
                    "Vmatrix" => ("‖", "‖", false),
                    "cases" => ("{", "", false),
                    "aligned" | "align" | "split" | "gathered" | "eqnarray" => ("", "", true),
                    _ => ("", "", false),
                };
                let m = Node::Matrix { rows, aligned, cases: env == "cases" };
                Some(if l.is_empty() && r.is_empty() {
                    m
                } else {
                    Node::Delim { left: l.into(), body: Box::new(m), right: r.into() }
                })
            }
            "," | "thinspace" => em(3.0 / 18.0),
            ":" | ">" | "medspace" => em(4.0 / 18.0),
            ";" | "thickspace" => em(5.0 / 18.0),
            "!" => em(-3.0 / 18.0),
            " " => em(0.33),
            "quad" => em(1.0),
            "qquad" => em(2.0),
            "displaystyle" | "textstyle" | "limits" | "nolimits" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr"
            | "Bigl" | "Bigr" | "nonumber" | "notag" => None,
            n if FUNCTIONS.contains(&n) => Some(Node::Sym(n.to_string(), Kind::Func)),
            n => match symbol(n) {
                Some((s, kind)) => {
                    let s = if kind == Kind::Ord && s.chars().count() == 1 {
                        italic(s.chars().next().unwrap()).to_string()
                    } else {
                        s.to_string()
                    };
                    Some(Node::Sym(s, kind))
                }
                None => Some(Node::Text(format!("\\{n}"))),
            },
        }
    }
}

fn parse(src: &str) -> Node {
    let mut p = Parser { c: src.chars().collect(), i: 0 };
    let mut rows = p.row(&["\\\\"]);
    // Top-level line breaks (e.g. several equations in one block) stack up.
    if p.starts("\\\\") {
        let mut lines = vec![vec![Node::Row(std::mem::take(&mut rows))]];
        while p.starts("\\\\") {
            p.i += 2;
            lines.push(vec![Node::Row(p.row(&["\\\\"]))]);
        }
        return Node::Matrix { rows: lines, aligned: false, cases: false };
    }
    Node::Row(rows)
}

// ------------------------------------------------------------------ layout

enum Item {
    Glyphs { at: Vec2, galley: Arc<Galley>, asc: f32 },
    Rule { rect: Rect },
    Line { a: Pos2, b: Pos2, width: f32 },
}

/// A laid-out box: `at` positions are relative to the left end of the baseline.
struct MathBox {
    w: f32,
    asc: f32,
    desc: f32,
    items: Vec<Item>,
}

impl MathBox {
    fn empty() -> Self {
        Self { w: 0.0, asc: 0.0, desc: 0.0, items: Vec::new() }
    }

    /// Put `other` at offset (dx, dy below baseline) inside self, growing it.
    fn place(&mut self, other: MathBox, dx: f32, dy: f32) {
        self.asc = self.asc.max(other.asc - dy);
        self.desc = self.desc.max(other.desc + dy);
        self.w = self.w.max(dx + other.w);
        for item in other.items {
            self.items.push(match item {
                Item::Glyphs { at, galley, asc } => Item::Glyphs { at: at + vec2(dx, dy), galley, asc },
                Item::Rule { rect } => Item::Rule { rect: rect.translate(vec2(dx, dy)) },
                Item::Line { a, b, width } => Item::Line { a: a + vec2(dx, dy), b: b + vec2(dx, dy), width },
            });
        }
    }
}

struct Layout<'a, 'f> {
    fonts: &'a mut FontsView<'f>,
    color: Color32,
}

impl Layout<'_, '_> {
    fn glyphs(&mut self, text: &str, size: f32) -> MathBox {
        if text.is_empty() {
            return MathBox::empty();
        }
        let galley = self.fonts.layout_no_wrap(text.to_string(), FontId::new(size, family()), self.color);
        let baseline = galley
            .rows
            .first()
            .and_then(|r| r.row.glyphs.first().map(|g| g.pos.y + r.pos.y))
            .unwrap_or(size * 0.8);
        let h = galley.size().y;
        // The font's line box is roomier than the ink; trim it for tighter stacking.
        let asc = baseline * 0.82;
        let desc = (h - baseline) * 0.55;
        MathBox { w: galley.size().x, asc, desc, items: vec![Item::Glyphs { at: Vec2::ZERO, galley, asc: baseline }] }
    }

    fn node(&mut self, n: &Node, size: f32, display: bool) -> MathBox {
        let script = (size * 0.7).max(8.0);
        match n {
            Node::Sym(s, Kind::Large) => {
                let big = if display { size * 1.6 } else { size * 1.2 };
                self.glyphs(s, big)
            }
            Node::Sym(s, _) => self.glyphs(s, size),
            Node::Text(s) => self.glyphs(s, size),
            Node::Space(em) => MathBox { w: em * size, asc: 0.0, desc: 0.0, items: Vec::new() },
            Node::Row(items) => self.row(items, size, display),
            Node::Frac(a, b) => {
                let inner = if display { size * 0.92 } else { (size * 0.75).max(8.0) };
                let num = self.node(a, inner, false);
                let den = self.node(b, inner, false);
                let t = (size * 0.055).max(1.0);
                let axis = size * 0.27;
                let gap = size * 0.14;
                let pad = size * 0.12;
                let w = num.w.max(den.w) + 2.0 * pad;
                let mut out = MathBox::empty();
                let num_shift = axis + t / 2.0 + gap + num.desc;
                let den_shift = -axis + t / 2.0 + gap + den.asc;
                let (nw, dw) = (num.w, den.w);
                out.place(num, (w - nw) / 2.0, -num_shift);
                out.place(den, (w - dw) / 2.0, den_shift);
                out.items.push(Item::Rule { rect: Rect::from_min_size(pos2(pad * 0.5, -axis - t / 2.0), vec2(w - pad, t)) });
                out.w = w;
                out
            }
            Node::Sqrt(index, body) => {
                let inner = self.node(body, size, display);
                let t = (size * 0.055).max(1.0);
                let gap = size * 0.12;
                let top = inner.asc + gap + t;
                let bottom = inner.desc.max(size * 0.1);
                let sign_w = size * 0.55;
                let mut out = MathBox::empty();
                let mut x0 = 0.0;
                if let Some(ix) = index {
                    let ixb = self.node(ix, (size * 0.5).max(7.0), false);
                    let iw = ixb.w;
                    out.place(ixb, 0.0, -top * 0.55);
                    x0 = (iw - sign_w * 0.4).max(0.0);
                }
                let (h_mid, h_bot) = (-top * 0.45, bottom);
                let lw = t * 1.1;
                out.items.push(Item::Line { a: pos2(x0, h_mid + size * 0.08), b: pos2(x0 + sign_w * 0.28, h_mid), width: lw });
                out.items.push(Item::Line { a: pos2(x0 + sign_w * 0.28, h_mid), b: pos2(x0 + sign_w * 0.55, h_bot), width: lw * 1.4 });
                out.items.push(Item::Line { a: pos2(x0 + sign_w * 0.55, h_bot), b: pos2(x0 + sign_w, -top + t / 2.0), width: lw });
                let iw = inner.w;
                out.place(inner, x0 + sign_w + size * 0.06, 0.0);
                out.items.push(Item::Rule {
                    rect: Rect::from_min_size(pos2(x0 + sign_w - t / 2.0, -top), vec2(iw + size * 0.16, t)),
                });
                out.asc = out.asc.max(top + t);
                out.desc = out.desc.max(bottom);
                out.w += size * 0.06;
                out
            }
            Node::Scripts { base, sub, sup } => {
                let b = self.node(base, size, display);
                let limits = display
                    && matches!(&**base, Node::Sym(s, Kind::Large | Kind::Func) if takes_limits(s));
                let sub_b = sub.as_ref().map(|s| self.node(s, script, false));
                let sup_b = sup.as_ref().map(|s| self.node(s, script, false));
                let mut out = MathBox::empty();
                if limits {
                    let w = b.w.max(sub_b.as_ref().map_or(0.0, |s| s.w)).max(sup_b.as_ref().map_or(0.0, |s| s.w));
                    let (basc, bdesc, bw) = (b.asc, b.desc, b.w);
                    out.place(b, (w - bw) / 2.0, 0.0);
                    if let Some(s) = sup_b {
                        let (sw, sd) = (s.w, s.desc);
                        out.place(s, (w - sw) / 2.0, -(basc + sd + size * 0.12));
                    }
                    if let Some(s) = sub_b {
                        let (sw, sa) = (s.w, s.asc);
                        out.place(s, (w - sw) / 2.0, bdesc + sa + size * 0.1);
                    }
                    out.w = w;
                    return out;
                }
                let (basc, bdesc, bw) = (b.asc, b.desc, b.w);
                let italic_kern = if matches!(&**base, Node::Sym(s, Kind::Large) if s == "∫" || s == "∮") { size * 0.15 } else { 0.0 };
                out.place(b, 0.0, 0.0);
                let x = bw + size * 0.04;
                let mut sup_shift = (basc - script * 0.38).max(size * 0.36);
                let mut sub_shift = (bdesc + script * 0.2).max(size * 0.18);
                if let (Some(sp), Some(sb)) = (&sup_b, &sub_b) {
                    // Keep a gap between them.
                    let clash = (sp.desc - sup_shift) + (sb.asc - sub_shift) + size * 0.12;
                    if clash > 0.0 {
                        sup_shift += clash / 2.0;
                        sub_shift += clash / 2.0;
                    }
                }
                if let Some(s) = sup_b {
                    out.place(s, x + italic_kern, -sup_shift);
                }
                if let Some(s) = sub_b {
                    out.place(s, x, sub_shift);
                }
                out.w += size * 0.04;
                out
            }
            Node::Accent(body, accent) => {
                let inner = self.node(body, size, display);
                let mut out = MathBox::empty();
                let (iw, iasc) = (inner.w, inner.asc);
                out.place(inner, 0.0, 0.0);
                let t = (size * 0.05).max(1.0);
                match accent {
                    Accent::Bar => {
                        out.items.push(Item::Rule {
                            rect: Rect::from_min_size(pos2(size * 0.05, -iasc - size * 0.1), vec2((iw - size * 0.1).max(size * 0.3), t)),
                        });
                        out.asc = out.asc.max(iasc + size * 0.1 + t);
                    }
                    other => {
                        let mark = match other {
                            Accent::Hat => "ˆ",
                            Accent::Tilde => "˜",
                            Accent::Vec => "→",
                            Accent::Dot => "˙",
                            _ => "¨",
                        };
                        let msize = size * if matches!(other, Accent::Vec) { 0.6 } else { 0.9 };
                        let m = self.glyphs(mark, msize);
                        let mw = m.w;
                        // Spacing marks carry their ink about half an em up:
                        // drop the mark so its ink sits just above the body.
                        let ink = if matches!(other, Accent::Vec) { 0.25 } else { 0.5 };
                        let lift = iasc + size * 0.04 - ink * msize;
                        out.place(m, (iw - mw) / 2.0 + size * 0.05, -lift);
                    }
                }
                out
            }
            Node::Delim { left, body, right } => {
                let inner = self.node(body, size, display);
                let axis = size * 0.27;
                let half = (inner.asc - axis).max(inner.desc + axis) + size * 0.08;
                let dsize = (half * 2.0 * 0.95).max(size);
                let mut out = MathBox::empty();
                let mut x = 0.0;
                let put = |out: &mut MathBox, this: &mut Self, s: &str, x: &mut f32| {
                    if s.is_empty() {
                        return;
                    }
                    let g = this.glyphs(s, dsize);
                    // Centre the delimiter on the math axis.
                    let shift = (g.asc - g.desc) / 2.0 - axis;
                    let w = g.w;
                    out.place(g, *x, shift);
                    *x += w;
                };
                put(&mut out, self, left, &mut x);
                let iw = inner.w;
                out.place(inner, x + size * 0.05, 0.0);
                x += iw + size * 0.1;
                put(&mut out, self, right, &mut x);
                out.w = x;
                out
            }
            Node::Matrix { rows, aligned, cases } => {
                let cells: Vec<Vec<MathBox>> =
                    rows.iter().map(|r| r.iter().map(|c| self.node(c, size, display)).collect()).collect();
                let ncols = cells.iter().map(Vec::len).max().unwrap_or(0);
                let mut widths = vec![0.0f32; ncols];
                for r in &cells {
                    for (j, c) in r.iter().enumerate() {
                        widths[j] = widths[j].max(c.w);
                    }
                }
                let col_gap = if *aligned { size * 0.25 } else { size * 0.9 };
                let row_gap = size * 0.35;
                let heights: Vec<(f32, f32)> = cells
                    .iter()
                    .map(|r| (r.iter().map(|c| c.asc).fold(size * 0.7, f32::max), r.iter().map(|c| c.desc).fold(size * 0.2, f32::max)))
                    .collect();
                let total: f32 = heights.iter().map(|(a, d)| a + d).sum::<f32>() + row_gap * (heights.len().saturating_sub(1)) as f32;
                let axis = size * 0.27;
                let mut y = -(total / 2.0) - axis;
                let mut out = MathBox::empty();
                for (r, (asc, desc)) in cells.into_iter().zip(&heights) {
                    y += asc;
                    let mut x = 0.0;
                    for (j, c) in r.into_iter().enumerate() {
                        let cw = c.w;
                        // aligned: right then left around each &; matrices centre.
                        let dx = if *cases {
                            0.0
                        } else if *aligned {
                            if j % 2 == 0 && ncols > 1 { widths[j] - cw } else { 0.0 }
                        } else {
                            (widths[j] - cw) / 2.0
                        };
                        out.place(c, x + dx, y);
                        x += widths[j] + if *aligned && j % 2 == 0 { size * 0.1 } else { col_gap };
                    }
                    y += desc + row_gap;
                }
                out.w = widths.iter().sum::<f32>() + col_gap * (ncols.saturating_sub(1)) as f32;
                out
            }
        }
    }

    fn row(&mut self, items: &[Node], size: f32, display: bool) -> MathBox {
        let mut out = MathBox::empty();
        let mut x = 0.0;
        let mut prev: Option<Kind> = None;
        let script_level = size < 11.0;
        for (i, n) in items.iter().enumerate() {
            let kind = n.kind();
            // A binary operator at the start, or after an operator, is unary.
            let kind = if kind == Kind::Bin && (prev.is_none() || matches!(prev, Some(Kind::Bin | Kind::Rel | Kind::Open | Kind::Punct))) {
                Kind::Ord
            } else {
                kind
            };
            let next_is_open = matches!(items.get(i + 1).map(Node::kind), Some(Kind::Open));
            let space = match (prev, kind) {
                _ if script_level => 0.0,
                (Some(_), Kind::Bin) | (Some(Kind::Bin), _) => 4.0 / 18.0,
                (Some(_), Kind::Rel) | (Some(Kind::Rel), _) => 5.0 / 18.0,
                (Some(Kind::Punct), _) => 3.0 / 18.0,
                (Some(Kind::Func), Kind::Ord) => 3.0 / 18.0,
                (Some(Kind::Large), Kind::Ord) => 2.0 / 18.0,
                (Some(Kind::Ord), Kind::Func | Kind::Large) => 3.0 / 18.0,
                _ => 0.0,
            };
            let _ = next_is_open;
            x += space * size;
            let b = self.node(n, size, display);
            let w = b.w;
            out.place(b, x, 0.0);
            x += w;
            prev = Some(kind);
        }
        out.w = x;
        out
    }
}

/// Lay out and draw display math, centred in the available width.
/// Returns the allocated response so callers can attach a tooltip.
pub fn show(ui: &mut Ui, src: &str, size: f32, color: Color32) -> egui::Response {
    let tree = parse(src.trim());
    let b = ui.fonts_mut(|fonts| Layout { fonts, color }.node(&tree, size, true));
    let pad = size * 0.25;
    let width = ui.available_width();
    let desired = vec2(width, b.asc + b.desc + 2.0 * pad);
    let (rect, resp) = ui.allocate_exact_size(desired, egui::Sense::hover());
    // Too wide: start at the left (the frame around scrolls horizontally).
    let x0 = if b.w < width { rect.left() + (width - b.w) / 2.0 } else { rect.left() };
    let origin = pos2(x0, rect.top() + pad + b.asc);
    let painter = ui.painter();
    for item in b.items {
        match item {
            Item::Glyphs { at, galley, asc } => painter.galley(origin + at - vec2(0.0, asc), galley, color),
            Item::Rule { rect } => {
                painter.rect_filled(rect.translate(origin.to_vec2()), 0.0, color);
            }
            Item::Line { a, b, width } => {
                painter.line_segment([origin + a.to_vec2(), origin + b.to_vec2()], Stroke::new(width, color));
            }
        }
    }
    resp
}

/// The width display math needs, for horizontal scrolling.
pub fn width(ui: &Ui, src: &str, size: f32) -> f32 {
    let tree = parse(src.trim());
    ui.fonts_mut(|fonts| Layout { fonts, color: Color32::WHITE }.node(&tree, size, true)).w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> String {
        format!("{:?}", parse(src))
    }

    #[test]
    fn parses_common_physics() {
        let t = kinds(r"R_{P/F}(m_{t\bar t}, m_t) = \frac{N_{pass}}{N_{fail}}");
        assert!(t.contains("Frac"));
        assert!(t.contains("Accent"));
        let t = kinds(r"\sum_{i=1}^{N} \sqrt{x_i^2 + \left(\frac{a}{b}\right)^2}");
        assert!(t.contains("Sqrt") && t.contains("Delim") && t.contains("Large"));
        let t = kinds(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}");
        assert!(t.contains("Matrix"));
        let t = kinds(r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & \text{otherwise} \end{cases}");
        assert!(t.contains("Matrix") && t.contains("otherwise"));
    }

    #[test]
    fn italic_letters() {
        assert_eq!(italic('a'), '𝑎');
        assert_eq!(italic('h'), 'ℎ');
        assert_eq!(italic('α'), '𝛼');
        assert_eq!(italic('1'), '1');
    }
}
