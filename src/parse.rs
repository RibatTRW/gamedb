//! Masking, function/string/symbol extraction, call-site scanning.
//! All indices are character offsets, never byte offsets: the whole parser runs
//! over `Vec<char>`, so a multi-byte character can never shift a line number.

use crate::rx::{self as m, *};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub const MAX_STRINGS_PER_FILE: usize = 5000;
pub const MAX_CALLEES_PER_FN: usize = 1000;

fn not_a_func() -> &'static HashSet<&'static str> {
    static S: OnceLock<HashSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| {
        [
            "if",
            "for",
            "while",
            "switch",
            "return",
            "sizeof",
            "do",
            "else",
            "catch",
            "case",
            "defined",
            "FUN",
            "foreach",
            "lock",
            "using",
            "fixed",
            "checked",
            "unchecked",
            "unsafe",
            "get",
            "set",
            "new",
            "stackalloc",
            "static",
        ]
        .into_iter()
        .collect()
    })
}

fn call_keywords() -> &'static HashSet<&'static str> {
    static S: OnceLock<HashSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| {
        [
            "if",
            "for",
            "while",
            "switch",
            "catch",
            "foreach",
            "using",
            "lock",
            "return",
            "do",
            "else",
            "case",
            "default",
            "new",
            "typeof",
            "sizeof",
            "nameof",
            "checked",
            "unchecked",
            "fixed",
            "stackalloc",
            "in",
            "is",
            "as",
            "yield",
            "await",
            "throw",
            "when",
            "let",
            "var",
        ]
        .into_iter()
        .collect()
    })
}

/// Blank comments (and by default string/char literals) with spaces, keeping
/// length and line numbers intact. `blank_literals=false` blanks comments only,
/// so a real literal can be told from one quoted inside a comment.
pub fn mask(s: &[char], blank_literals: bool) -> Vec<char> {
    let mut out = s.to_vec();
    let n = s.len();
    let mut i = 0;
    while i < n {
        let c = s[i];
        let nx = if i + 1 < n { s[i + 1] } else { '\0' };
        if c == '/' && nx == '/' {
            while i < n && s[i] != '\n' {
                out[i] = ' ';
                i += 1;
            }
        } else if c == '/' && nx == '*' {
            out[i] = ' ';
            out[i + 1] = ' ';
            i += 2;
            while i < n && !(s[i] == '*' && i + 1 < n && s[i + 1] == '/') {
                if s[i] != '\n' {
                    out[i] = ' ';
                }
                i += 1;
            }
            if i < n {
                out[i] = ' ';
                out[i + 1] = ' ';
                i += 2;
            }
        } else if blank_literals && (c == '"' || c == '\'') {
            let q = c;
            out[i] = ' ';
            i += 1;
            while i < n && s[i] != q {
                if s[i] != '\n' {
                    out[i] = ' ';
                }
                if s[i] == '\\' {
                    out[i] = ' ';
                    i += 1;
                }
                i += 1;
            }
            if i < n {
                out[i] = ' ';
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Character ranges of each line, end-exclusive. Shared by src/masked/no_comments
/// because masking never adds or removes a newline.
pub fn line_ranges(s: &[char]) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for (i, &c) in s.iter().enumerate() {
        if c == '\n' {
            v.push((start, i));
            start = i + 1;
        }
    }
    v.push((start, s.len()));
    v
}
fn trim_ws(s: &[char]) -> &[char] {
    let mut a = 0;
    let mut b = s.len();
    while a < b && is_ws(s[a]) && s[a] != '\n' {
        a += 1;
    }
    while b > a && is_ws(s[b - 1]) && s[b - 1] != '\n' {
        b -= 1;
    }
    &s[a..b]
}

/// Drop a CRLF's `\r` and any trailing blanks. A decompiler that wrote the
/// tree on Windows leaves `\r` at the end of every line, and every pattern below
/// is anchored on the last non-blank character, so it has to go.
pub fn chomp(s: &[char]) -> &[char] {
    let mut b = s.len();
    // CR only: JS `split(/\r?\n/)` leaves trailing spaces on each line, and the
    // reference's `sig` join keeps them.
    while b > 0 && s[b - 1] == '\r' {
        b -= 1;
    }
    &s[..b]
}

fn trim_ws_end(s: &[char]) -> &[char] {
    let mut b = s.len();
    while b > 0 && is_ws(s[b - 1]) {
        b -= 1;
    }
    &s[..b]
}

fn trim_line(s: &[char]) -> &[char] {
    trim_ws(chomp(s))
}

/// `"=>"`, the expression-bodied member marker. Adjacent, not two characters
/// that merely appear somewhere on the line.
fn has_arrow(s: &[char]) -> bool {
    s.windows(2).any(|w| w == ['=', '>'])
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone)]
pub struct Func {
    pub name: String,
    pub params: String,
    pub sig: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct Sym {
    pub kind: &'static str,
    pub name: String,
    pub line: usize,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub funcs: Vec<Func>,
    pub strings: Vec<(usize, String)>,
    pub syms: Vec<Sym>,
}

/// Assumes one-line signatures, true for Ghidra/IDA C and ILSpy C#. A
/// multi-line signature would need paren-aware joining.
pub fn parse_source(src: &[char]) -> Parsed {
    let masked = mask(src, true);
    let no_comments = mask(src, false);
    let lr = line_ranges(&masked);
    let mut out = Parsed::default();

    for (li, &(a, b)) in lr.iter().enumerate() {
        // Literals are read from the RAW line. Masking comments first would
        // destroy any literal containing `//` - which is every URL in the tree -
        // so `no_comments` is used only as the guard, at the opening quote.
        let mut lits = Vec::new();
        m::scan_str_lits(&src[a..b], &mut lits);
        for (s, e) in lits {
            if e <= s + 2 || no_comments[a + s] != '"' {
                continue;
            }
            let text: String = src[a + s + 1..a + e - 1].iter().collect();
            if text.chars().count() >= 4 && !is_hexish(&text) {
                out.strings.push((li + 1, text));
            }
        }
    }

    let mut li = 0usize;
    while li < lr.len() {
        let (a, b) = lr[li];
        let line = chomp(&masked[a..b]);
        if !line.contains(&'(') {
            li += 1;
            continue;
        }
        // Allman braces: `)` closes the header, `{` sits alone on the next line.
        let mut brace_line = li;
        let mut cand: Vec<char> = line.to_vec();
        if !line
            .iter()
            .rev()
            .find(|&&c| c != ' ' && c != '\t' && c != '\r')
            .is_some_and(|&c| c == '{')
        {
            let stripped = m::strip_where_throws(line);
            let bare = trim_ws_end(&stripped);
            let mut nx = li + 1;
            while nx < lr.len() && trim_ws(&masked[lr[nx].0..lr[nx].1]).is_empty() {
                nx += 1;
            }
            if nx >= lr.len() {
                li += 1;
                continue;
            }
            let nxt = trim_ws(&masked[lr[nx].0..lr[nx].1]);
            if !bare.last().is_some_and(|&c| c == ')') || nxt.len() != 1 || nxt[0] != '{' {
                li += 1;
                continue;
            }
            brace_line = nx;
            // Rejoin the header with the `{` that lives on its own line, so the
            // tail matcher sees the same shape it sees on one line.
            cand = bare.to_vec();
            cand.push(' ');
            cand.push('{');
        }

        let hit = m::match_header(&cand).or_else(|| m::match_csharp_header(&cand));
        let (name, params) = match hit {
            Some(v) => v,
            None => {
                li += 1;
                continue;
            }
        };
        if not_a_func().contains(name.as_str()) || name.starts_with("__") {
            li += 1;
            continue;
        }
        if m::has_close_paren_semi(&cand) {
            li += 1;
            continue;
        }

        // Body end: walk masked chars from the header's '{' to its match.
        let (ba, bb) = lr[brace_line];
        let mut depth = 0i32;
        let mut end = li;
        let mut nl = brace_line;
        let rel = masked[ba..bb].iter().rposition(|&c| c == '{');
        let start = match rel {
            Some(r) => ba + r,
            None => ba,
        };
        for &ch in &masked[start..] {
            match ch {
                '\n' => nl += 1,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = nl + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end < li + 1 {
            end = li + 1; // unbalanced tail: claim this line, never rewind
        }
        let mut sig_parts: Vec<String> = Vec::new();
        // rawLines are split on /\r?\n/, so a CRLF tree contributes no `\r` here
        for k in li..=brace_line {
            sig_parts.push(String::from_iter(chomp(&src[lr[k].0..lr[k].1])));
        }
        out.funcs.push(Func {
            name,
            params: collapse_ws(&params),
            sig: sig_parts.join(" ").trim().to_string(),
            start: li + 1,
            end,
        });
        li = end; // functions do not nest; resume after the body
    }

    // Member symbols: declarations outside any function body. Locals live inside
    // bodies, so "not inside one" separates members from noise.
    let mut fi = 0usize;
    for li in 0..lr.len() {
        while fi < out.funcs.len() && out.funcs[fi].end < li + 1 {
            fi += 1;
        }
        if fi < out.funcs.len() && out.funcs[fi].start <= li + 1 {
            continue;
        }
        let (a, b) = lr[li];
        let t = trim_line(&masked[a..b]);
        if t.is_empty() {
            continue;
        }
        if let Some(n) = m::match_namespace(t) {
            out.syms.push(Sym {
                kind: "namespace",
                name: n,
                line: li + 1,
            });
            continue;
        }
        if let Some(n) = m::match_type(t) {
            out.syms.push(Sym {
                kind: "type",
                name: n,
                line: li + 1,
            });
            continue;
        }
        let mem = match m::match_member(t) {
            Some(n) => n,
            None => continue,
        };
        // A property is a declaration followed by an accessor - `=>` on the same
        // line, or a lone Allman `{` and then `get`/`set`/`init`. A field has
        // `;` or `=` after the name and nothing else.
        let mut is_prop = has_arrow(t) || m::brace_then_accessor(t);
        let mut hops = 0;
        let mut k = li + 1;
        while !is_prop && k < lr.len() && hops < 3 {
            let u = trim_line(&masked[lr[k].0..lr[k].1]);
            if u.is_empty() {
                k += 1;
                continue;
            }
            hops += 1;
            if m::is_accessor_start(u) {
                is_prop = true;
                break;
            }
            if u.len() != 1 || u[0] != '{' {
                break;
            }
            k += 1;
        }
        out.syms.push(Sym {
            kind: if is_prop { "property" } else { "field" },
            name: mem,
            line: li + 1,
        });
    }
    out
}

/// Distinct callees in one body. Name-only resolution: no type information, so
/// `a.Bar()` lands on Bar and a shadowing local yields a false edge.
pub fn scan_calls(
    masked: &[char],
    lr: &[(usize, usize)],
    start: usize,
    end: usize,
    self_id: i64,
    by_name: &HashMap<String, Vec<i64>>,
) -> Vec<(i64, usize)> {
    let mut out = Vec::new();
    let mut seen: HashSet<i64> = HashSet::new();
    let hi = end.min(lr.len());
    for li in start..=hi {
        if li == 0 || li > lr.len() {
            continue;
        }
        let (a, b) = lr[li - 1];
        let mut sites = Vec::new();
        m::scan_calls(&masked[a..b], &mut sites);
        for (name, _) in sites {
            if call_keywords().contains(name.as_str()) {
                continue;
            }
            let dsts = match by_name.get(&name) {
                Some(v) => v,
                None => continue,
            };
            for &dst in dsts {
                if dst == self_id || seen.contains(&dst) {
                    continue;
                }
                seen.insert(dst);
                out.push((dst, li));
                if out.len() >= MAX_CALLEES_PER_FN {
                    return out;
                }
            }
        }
    }
    out
}

/// `NS_RE` over the whole file, for namespace fallback.
pub fn find_namespace(src: &[char]) -> Option<String> {
    m::find_namespace(src)
}
