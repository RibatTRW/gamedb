//! Hand-written matchers. No regex crate: this file *is* the pattern set, and
//! every character class is ASCII by construction. There is no code path by
//! which `\w` can mean "Unicode letters" the way a default regex engine allows,
//! so the parity hazard is structurally impossible rather than merely tested.

pub fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}' | '\u{b}')
}
pub fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
pub fn is_word_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}
/// `[\w<>\[\],.?*]` -- `*` is here so a pointer return type or a by-pointer
/// parameter does not abort the C# prefix scan before it reaches the name.
pub fn is_tok(c: char) -> bool {
    is_word(c) || matches!(c, '<' | '>' | '[' | ']' | ',' | '.' | '?' | '*')
}

/// True for the tokens that sit *between* a return type and the function's name:
/// calling conventions (MSVC, GCC/Clang itanium, MinGW), storage classes and
/// inlining hints. Decompilers emit these unasked - `undefined4 * __thiscall
/// FUN_004087c6(void)` is Ghidra's standard shape - and a matcher that assumes
/// "second token is the name" silently drops every one of them.
///
/// The rule is structural rather than a fixed list: any `__`-prefixed
/// convention spelling ends in `call` (`__stdcall`, `__fastcall`, `__thiscall`,
/// `__vectorcall`, `__clrcall`, `__swiftcall`, ...) or `decl` (`__cdecl`). The
/// explicit cases cover the uppercase Windows spellings, which have no prefix.
pub fn is_modifier_tok(w: &str) -> bool {
    if w.starts_with("__") && (w.ends_with("call") || w.ends_with("decl")) {
        return true;
    }
    matches!(
        w,
        "WINAPI"
            | "APIENTRY"
            | "CALLBACK"
            | "extern"
            | "static"
            | "inline"
            | "__inline"
            | "__forceinline"
            | "noinline"
            | "__declspec"
    )
}

/// Character offset of the `)` that closes the `(` at `open`, counting nested
/// parens. Returns `None` if they never balance.
///
/// Deliberately paren-aware rather than "scan to the first `)`": a
/// function-pointer parameter (`int (*cb)(int)`) or a nested generic would
/// otherwise close the scan early and the whole header would be dropped.
fn close_paren(s: &[char], open: usize) -> Option<(usize, String)> {
    let mut depth = 0i32;
    let mut i = open;
    while i < s.len() {
        match s[i] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((i, s[open + 1..i].iter().collect()));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn read_word(s: &[char], i: usize) -> Option<(String, usize)> {
    if i >= s.len() || !is_word_start(s[i]) {
        return None;
    }
    let mut j = i;
    while j < s.len() && is_word(s[j]) {
        j += 1;
    }
    Some((s[i..j].iter().collect(), j))
}

fn skip_ws(s: &[char], mut i: usize) -> usize {
    while i < s.len() && is_ws(s[i]) {
        i += 1;
    }
    i
}

fn at_end(s: &[char], mut i: usize) -> bool {
    while i < s.len() && is_ws(s[i]) {
        i += 1;
    }
    i == s.len()
}

/// `(NAME) (<...>)? \s* \( ([^()]*) \) \s* { \s* $`
fn tail(s: &[char], i: usize) -> Option<(String, String)> {
    let (name, j) = read_word(s, i)?;
    let mut k = skip_ws(s, j);
    if k < s.len() && s[k] == '<' {
        k += 1;
        while k < s.len() && s[k] != '>' {
            k += 1;
        }
        if k >= s.len() {
            return None;
        }
        k = skip_ws(s, k + 1);
    }
    if k >= s.len() || s[k] != '(' {
        return None;
    }
    let (pe, params) = close_paren(s, k)?;
    let k = skip_ws(s, pe + 1);
    if k >= s.len() || s[k] != '{' {
        return None;
    }
    if !at_end(s, k + 1) {
        return None;
    }
    Some((name, params))
}

/// `HEADER`: optional `Type <*>* `, optional modifier words, then the tail,
/// then an optional `/*...*/` comment.
pub fn match_header(s: &[char]) -> Option<(String, String)> {
    // ^\s*(?:WORD(?:\s*\*)*\s+(?:MOD\s+)*)?(WORD)\s*\(([^()]*)\)\s*(?:/\*.*)?\{\s*$
    let p = skip_ws(s, 0);
    // Storage and calling-class keywords may lead the whole declaration rather
    // than sit in the name slot: `static inline int f(`, `extern void g(`.
    // Skipping them first is what lets the type word below be the type.
    let mut head = p;
    loop {
        let r = skip_ws(s, head);
        match read_word(s, r) {
            Some((w, j)) if is_modifier_tok(&w) => head = j,
            _ => break,
        }
    }
    let head = skip_ws(s, head);
    // Optional `(?:TYPE\s*\*\s+)?`: pointer stars live *before* the name and must be
    // separated from it by whitespace, so `char **foo(` is deliberately rejected.
    let mut name_at = head;
    if head < s.len() && is_word_start(s[head]) {
        if let Some((_, after)) = read_word(s, head) {
            let mut q = after;
            loop {
                let r = skip_ws(s, q);
                if r < s.len() && s[r] == '*' {
                    q = r + 1;
                } else {
                    break;
                }
            }
            // Calling conventions and storage hints sit in the same slot as a
            // second type word: `void __thiscall f(`, `static inline int g(`.
            loop {
                let r = skip_ws(s, q);
                if r < s.len() && s[r] == '*' {
                    q = r + 1;
                    continue;
                }
                match read_word(s, r) {
                    Some((w, j)) if r > q && is_modifier_tok(&w) => q = j,
                    _ => break,
                }
            }
            let k = skip_ws(s, q);
            if k > q && k < s.len() && is_word_start(s[k]) {
                name_at = k;
            }
        }
    }
    if name_at >= s.len() || !is_word_start(s[name_at]) {
        return None;
    }
    let (name, j) = read_word(s, name_at)?;
    let k = skip_ws(s, j);
    if k >= s.len() || s[k] != '(' {
        return None;
    }
    let (close, params) = close_paren(s, k)?;
    // only `\s*` and an optional `/* */` may sit between `)` and `{` - the regex
    // does not tolerate a trailing `throws ...`, which is why Java constructors
    // that declare one are not indexed.
    let mut e = skip_ws(s, close + 1);
    if e + 1 < s.len() && s[e] == '/' && s[e + 1] == '*' {
        e = s.len();
    }
    if e >= s.len() || s[e] != '{' || !at_end(s, e + 1) {
        return None;
    }
    Some((name, collapse_ws(&params)))
}

/// `m[2].trim().replace(/\s+/g, " ")`
pub fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `CSHARP_HEADER`: `(?:[\w<>\[\],.?]+\s+)*?` then the tail. Non-greedy, so
/// the earliest viable name start wins.
pub fn match_csharp_header(s: &[char]) -> Option<(String, String)> {
    let i0 = skip_ws(s, 0);
    let mut i = i0;
    loop {
        if i >= s.len() {
            return None;
        }
        if i == i0 || is_ws(s[i - 1]) {
            if let Some(hit) = tail(s, i) {
                return Some(hit);
            }
        }
        if is_tok(s[i]) {
            i += 1;
        } else if is_ws(s[i]) {
            i = skip_ws(s, i);
        } else {
            return None;
        }
    }
}

/// `namespace\s+([\w.]+)` — greedy through dots, so `A.B.C` is one name.
pub fn match_namespace(s: &[char]) -> Option<String> {
    const KW: &str = "namespace";
    let mut i = skip_ws(s, 0);
    for c in KW.chars() {
        if i >= s.len() || s[i] != c {
            return None;
        }
        i += 1;
    }
    let j = skip_ws(s, i);
    if j == i {
        return None; // `\s+` is required, so `namespace.Foo()` is not a declaration
    }
    let mut k = j;
    while k < s.len() && (is_word(s[k]) || s[k] == '.') {
        k += 1;
    }
    while k > j && s[k - 1] == '.' {
        k -= 1;
    }
    (k > j).then(|| s[j..k].iter().collect())
}

/// `CS_TYPE`: `^(?:[\w<>\[\],.?]+\s+)*(?:class|...)\s+([A-Za-z_]\w*)`. The
/// prefix is greedy, so the *last* viable keyword inside one contiguous
/// token+whitespace run at position 0 wins; a keyword further along the line
/// (after a `(` or `;`) is not a declaration.
pub fn match_type(s: &[char]) -> Option<String> {
    const KWS: [&str; 6] = ["class", "struct", "interface", "enum", "record", "delegate"];
    let mut ends: Vec<usize> = vec![0]; // zero repetitions is the last fallback
    let mut i = 0;
    loop {
        let mut j = i;
        while j < s.len() && is_tok(s[j]) {
            j += 1;
        }
        if j == i {
            break;
        }
        let k = skip_ws(s, j);
        if k == j {
            break;
        }
        ends.push(k);
        i = k;
    }
    for &p in ends.iter().rev() {
        for kw in KWS {
            let k: Vec<char> = kw.chars().collect();
            if p + k.len() <= s.len() && s[p..p + k.len()] == k[..] {
                let j = skip_ws(s, p + k.len());
                if j > p + k.len() && j < s.len() && is_word_start(s[j]) {
                    if let Some((n, _)) = read_word(s, j) {
                        return Some(n);
                    }
                }
            }
        }
    }
    None
}

const MODIFIERS: [&str; 20] = [
    "public",
    "private",
    "protected",
    "internal",
    "static",
    "readonly",
    "const",
    "volatile",
    "virtual",
    "override",
    "abstract",
    "sealed",
    "partial",
    "new",
    "unsafe",
    "extern",
    "required",
    "event",
    "implicit",
    "explicit",
];

/// `CS_MEMBER`: `(?:MODIFIER)\s+(?:tok\s+)*?(NAME)\s*(?=$|[=;{])`, non-greedy.
pub fn match_member(s: &[char]) -> Option<String> {
    let mut p = None;
    for kw in MODIFIERS {
        let k: Vec<char> = kw.chars().collect();
        if s.len() >= k.len() && s[..k.len()] == k[..] {
            let j = skip_ws(s, k.len());
            if j > k.len() {
                p = Some(j);
                break;
            }
        }
    }
    let p = p?;
    let mut i = p;
    loop {
        if i >= s.len() {
            return None;
        }
        if (i == p || is_ws(s[i - 1])) && is_word_start(s[i]) {
            if let Some((name, j)) = read_word(s, i) {
                let e = skip_ws(s, j);
                if e == s.len() || matches!(s[e], '=' | ';' | '{') {
                    return Some(name);
                }
            }
        }
        if is_tok(s[i]) {
            i += 1;
        } else if is_ws(s[i]) {
            i = skip_ws(s, i);
        } else {
            return None;
        }
    }
}

/// `CALL_SCAN` + `CALL_KEYWORDS`: `(name, start_index)` for every `name(` site.
pub fn scan_calls(s: &[char], out: &mut Vec<(String, usize)>) {
    let mut i = 0;
    while i < s.len() {
        if is_word_start(s[i]) && (i == 0 || !is_word(s[i - 1])) {
            if let Some((name, j)) = read_word(s, i) {
                let k = skip_ws(s, j);
                if k < s.len() && s[k] == '(' {
                    out.push((name, i));
                }
            }
        }
        i += 1;
    }
}

/// `STR_LIT`: `(start, end)` of each complete `"..."` literal, quotes included.
/// A backslash escape may not span a newline, so an unterminated literal fails.
pub fn scan_str_lits(s: &[char], out: &mut Vec<(usize, usize)>) {
    let mut i = 0;
    while i < s.len() {
        if s[i] == '"' {
            let mut j = i + 1;
            let mut ok = false;
            while j < s.len() && s[j] != '\n' {
                if s[j] == '\\' {
                    if j + 1 < s.len() && s[j + 1] != '\n' {
                        j += 2;
                        continue;
                    }
                    break;
                }
                if s[j] == '"' {
                    ok = true;
                    break;
                }
                j += 1;
            }
            if ok {
                out.push((i, j + 1));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
}

/// `^[0-9a-fA-F\s]+$` — a "hex literal" that is really just a checksum.
pub fn is_hexish(t: &str) -> bool {
    !t.is_empty() && t.chars().all(|c| c.is_ascii_hexdigit() || is_ws(c))
}

/// `\)\s*;`
pub fn has_close_paren_semi(s: &[char]) -> bool {
    let mut i = 0;
    while i < s.len() {
        if s[i] == ')' {
            let j = skip_ws(s, i + 1);
            if j < s.len() && s[j] == ';' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// `\s+(?:where|throws)\s+[\w\s:<>,.()]+$`, cut at the first match.
pub fn strip_where_throws(s: &[char]) -> Vec<char> {
    let mut i = 0;
    while i < s.len() {
        if is_ws(s[i]) {
            let j = skip_ws(s, i);
            for kw in ["where", "throws"] {
                let k: Vec<char> = kw.chars().collect();
                if j + k.len() <= s.len() && s[j..j + k.len()] == k[..] {
                    let e = skip_ws(s, j + k.len());
                    if e > j + k.len()
                        // the class is [\w\s:<>,.()] - `:` carries the `where T : ...`
                        && (0..s.len() - e).all(|o| {
                            let c = s[e + o];
                            is_word(c)
                                || is_ws(c)
                                || matches!(c, ':' | '<' | '>' | ',' | '.' | '(' | ')')
                        })
                    {
                        let mut head: Vec<char> = s[..i].to_vec();
                        while head.last().is_some_and(|&c| c != '\n' && is_ws(c)) {
                            head.pop();
                        }
                        return head;
                    }
                }
            }
        }
        i += 1;
    }
    s.to_vec()
}

/// `\{\s*(get|set|init)\b`
pub fn brace_then_accessor(s: &[char]) -> bool {
    let mut i = 0;
    while i < s.len() {
        if s[i] == '{' {
            let j = skip_ws(s, i + 1);
            for kw in ["get", "set", "init"] {
                let k: Vec<char> = kw.chars().collect();
                if j + k.len() <= s.len() && s[j..j + k.len()] == k[..] {
                    let e = j + k.len();
                    if e == s.len() || !is_word(s[e]) {
                        return true;
                    }
                }
            }
        }
        i += 1;
    }
    false
}

/// `^(get|set|init)\b`
pub fn is_accessor_start(s: &[char]) -> bool {
    for kw in ["get", "set", "init"] {
        let k: Vec<char> = kw.chars().collect();
        if s.len() >= k.len()
            && s[..k.len()] == k[..]
            && (s.len() == k.len() || !is_word(s[k.len()]))
        {
            return true;
        }
    }
    false
}
