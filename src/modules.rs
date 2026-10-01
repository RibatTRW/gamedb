//! Module designation: which named slice of the codebase a file belongs to.
//!
//! Design note -- *nothing here knows what game it is indexing*. A rule set is
//! data. The shipped default is **derived from the corpus being indexed**, and
//! the only way a hand-written taxonomy exists is a plain text file the operator
//! drops in `.gamedb/modules.txt` (or points `--modules` at). That keeps the
//! feature useful on the first run of a codebase nobody has seen before instead
//! of inert, and it keeps project-specific knowledge in the project rather than
//! in the tool.
//!
//! Rule file format, one rule per line, `#` starts a comment, columns separated
//! by TAB (or runs of spaces if there is no TAB):
//!
//! ```text
//! n:Engine.Rendering   core.rendering   Graphics & rendering
//! b:MainLoop.cs        core.main        Main loop & state
//! x:Engine             core.runtime     Root namespace primitives
//! m:Assets             content.assets   Asset tree
//! n:Engine.Rendering @depth=3            more specific, wins
//! ```
//!
//! Selector prefixes: `n:` namespace prefix (matches the declared namespace and
//! the directory-chain fallback), `b:` basename, `x:` exact namespace, `m:` bare
//! path segment. `@depth=N` overrides how many namespace segments must match;
//! the default is 2.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const STATE_UNIMPLEMENTED: &str = "UNIMPLEMENTED";
pub const STATE_PARTIAL: &str = "PARTIALLY_IMPLEMENTED";
pub const STATE_IMPLEMENTED: &str = "IMPLEMENTED";
pub const MODULE_STATES: [&str; 3] = [STATE_UNIMPLEMENTED, STATE_PARTIAL, STATE_IMPLEMENTED];

/// Default name-segmentation depth for derived rules: the first two namespace
/// segments, which is the near-universal directory convention.
pub const DERIVE_DEPTH: usize = 2;
pub const DERIVE_MAX_MODULES: usize = 60;
pub const DERIVE_MIN_FILES: usize = 5;

/// One designation rule. `needle` keeps its `n:`/`b:`/`x:`/`m:` prefix so a
/// round-trip through the rule file never loses meaning.
pub struct Rule {
    pub needle: String,
    pub module_id: String,
    pub system: String,
    pub depth: u8,
    /// position in the file; earlier wins ties, so catch-alls belong at the end
    pub order: usize,
}

pub struct RuleSet {
    pub rules: Vec<Rule>,
    /// where the rules came from, surfaced by `stats` so nobody has to guess
    pub source: String,
}

impl RuleSet {
    /// Best rule for `rel` (repo-relative, `/`-separated), consulting the
    /// declared `src` when available. Returns `(module_id, system)`.
    ///
    /// Ranking is `(selector kind, width, -order)`: a rule that pins the file
    /// itself (`b:`) outranks one that pins an exact namespace (`x:`), which
    /// outranks a namespace prefix (`n:`), which outranks a bare path segment
    /// (`m:`). At equal kind, the rule that consumed more segments wins. File
    /// order is the last tie-break only.
    ///
    /// That ordering is the whole point: it is why a two-segment `n:A.B`
    /// outranks a catch-all `x:A` no matter which order the two lines appear
    /// in, and why an explicit `b:Player.cs` outranks the `n:` rule that also
    /// happens to cover that file. Ordering rules by hand in the file is a
    /// losing game; ranking them is not.
    pub fn module_of<'a>(&'a self, rel: &str, src: Option<&str>) -> (&'a str, &'a str) {
        let ns = namespace_of(rel, src);
        let base = Path::new(rel)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let segs: Vec<&str> = ns.split('.').filter(|s| !s.is_empty()).collect();
        let path_segs: Vec<&str> = rel
            .split(['/', '\\'])
            .filter(|s| !s.is_empty() && *s != ".")
            .collect();

        let mut best: Option<((i32, i32, i32), &Rule)> = None;
        for r in &self.rules {
            let Some(matched) = r.match_depth(&segs, &base, &path_segs, &ns) else {
                continue;
            };
            let cand = (matched.0, matched.1, -(r.order as i32));
            if best.as_ref().is_none_or(|(b, _)| cand > *b) {
                best = Some((cand, r));
            }
        }
        match best {
            Some((_, r)) => (&r.module_id, &r.system),
            None => ("unmapped", "UNMAPPED -- no rule matched"),
        }
    }

    pub fn is_known(&self, module_id: &str) -> bool {
        module_id == "unmapped" || self.rules.iter().any(|r| r.module_id == module_id)
    }

    /// The `system` label attached to a module id, taken from its first rule.
    pub fn system_of(&self, module_id: &str) -> Option<&str> {
        self.rules
            .iter()
            .find(|r| r.module_id == module_id)
            .map(|r| r.system.as_str())
    }

    pub fn counts(&self) -> (usize, usize) {
        let ids: std::collections::HashSet<&str> =
            self.rules.iter().map(|r| r.module_id.as_str()).collect();
        (self.rules.len(), ids.len())
    }
}

impl Rule {
    /// `(selector rank, segments consumed)` when this rule fires, or `None` when
    /// it does not apply. The rank encodes how strong a statement the rule
    /// makes: naming the file beats naming its namespace beats naming a
    /// directory; width breaks ties inside one kind.
    fn match_depth(
        &self,
        segs: &[&str],
        base: &str,
        path_segs: &[&str],
        ns: &str,
    ) -> Option<(i32, i32)> {
        let (kind, pat) = match self.needle.split_once(':') {
            Some((k, p)) => (k, p),
            None => ("n", self.needle.as_str()),
        };
        match kind {
            // the file itself: the strongest thing a rule can name
            "b" => (base == pat).then_some((3, 1)),
            // the namespace and nothing under it
            "x" => (ns == pat).then_some((2, segs.len() as i32)),
            // a directory: the weakest selector, a fallback bucket
            "m" => path_segs.iter().position(|s| *s == pat).map(|i| {
                (
                    0,
                    path_segs[..i].iter().filter(|s| !s.contains('.')).count() as i32,
                )
            }),
            // a bare `Demo.Engine` with no prefix means the same as `n:Demo.Engine`
            _ => {
                let ps: Vec<&str> = pat.split('.').filter(|s| !s.is_empty()).collect();
                if ps.is_empty() || ps.len() > segs.len() {
                    return None;
                }
                if ps.iter().zip(segs).any(|(a, b)| a != b) {
                    return None;
                }
                // `@depth=N` lets a rule claim a match it would otherwise be
                // too wide for, and be ranked as if it were only N deep.
                Some((1, self.depth.min(ps.len() as u8) as i32))
            }
        }
    }
}

/// The first namespace declared in `src`, or the directory chain of `rel` dotted
/// together (`a/b/C.cs` -> `a.b`) when the file declares none.
pub fn namespace_of(rel: &str, src: Option<&str>) -> String {
    if let Some(text) = src {
        for line in text.lines().take(64) {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("namespace") {
                // `namespaceFoo` is an identifier, not a declaration.
                if !rest.starts_with(char::is_whitespace) {
                    continue;
                }
                let name: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '_')
                    .collect();
                let name = name.trim_end_matches('.').to_string();
                if !name.is_empty()
                    && (name.contains('.')
                        || name.chars().next().is_some_and(|c| c.is_alphabetic()))
                {
                    return name;
                }
            }
        }
    }
    rel.split(['/', '\\'])
        .filter(|s| !s.is_empty() && *s != "." && Path::new(s).extension().is_none())
        .collect::<Vec<_>>()
        .join(".")
}

/// Parse a rule file. `path` is only used for error messages.
pub fn parse_rule_file(
    path: &Path,
    text: &str,
) -> Result<Vec<(String, String, String, u8)>, String> {
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let lineno = n + 1;
        let mut line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // An optional trailing `@depth=N`, separated by any whitespace so a
        // TAB-separated file does not need a space before it too.
        let mut depth = DERIVE_DEPTH as u8;
        if let Some((head, tail)) = line.rsplit_once(char::is_whitespace) {
            let tail = tail.trim();
            if let Some(n) = tail.strip_prefix("@depth=") {
                let v: usize = n.trim().parse().map_err(|_| {
                    format!(
                        "{}:{}: @depth= must be a number, got {:?}",
                        path.display(),
                        lineno,
                        n.trim()
                    )
                })?;
                if !(1..=8).contains(&v) {
                    return Err(format!(
                        "{}:{}: @depth must be 1..8",
                        path.display(),
                        lineno
                    ));
                }
                depth = v as u8;
                line = head.trim_end();
            }
        }
        let tabbed = line.contains('\t');
        // Empty fields are dropped rather than rejected: a rule file a human
        // aligned with tab stops (`n:X\t\t\tcore.x\t\tRuntime`) is the natural
        // thing to write, and refusing it would be the parser being precious
        // about whitespace instead of about the rule.
        let cols: Vec<&str> = if tabbed {
            line.split('\t')
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .collect()
        } else {
            line.split_whitespace().collect()
        };
        if cols.len() < 2 {
            return Err(format!(
                "{}:{}: need `needle<TAB>module_id[<TAB>system]`, got {:?}",
                path.display(),
                lineno,
                raw
            ));
        }
        let system = if cols.len() >= 3 {
            cols[2].to_string()
        } else {
            "User-defined module".to_string()
        };
        out.push((cols[0].to_string(), cols[1].to_string(), system, depth));
    }
    Ok(out)
}

fn rule_set_from(rows: Vec<(String, String, String, u8)>, source: String) -> RuleSet {
    RuleSet {
        rules: rows
            .into_iter()
            .enumerate()
            .map(|(order, (needle, module_id, system, depth))| Rule {
                needle,
                module_id,
                system,
                depth,
                order,
            })
            .collect(),
        source,
    }
}

/// Path of the operator-owned rules file, if present.
pub fn default_rules_path(root: &Path) -> PathBuf {
    root.join(".gamedb").join("modules.txt")
}

/// Pick the rule set to index with.
///
/// * `spec` empty -- `<root>/.gamedb/modules.txt` if it exists, otherwise derived
/// * `spec` `derive` -- force derivation, ignoring any rules file
/// * `spec` a path -- that file, or an error
pub fn load_rule_set(root: &Path, spec: &str, paths: &[String]) -> Result<RuleSet, String> {
    let file = default_rules_path(root);
    if spec.is_empty() {
        if let Ok(text) = std::fs::read_to_string(&file) {
            let rows = parse_rule_file(&file, &text)?;
            return Ok(rule_set_from(rows, file.display().to_string()));
        }
        return Ok(derive_rule_set(root, paths));
    }
    if spec.eq_ignore_ascii_case("derive") {
        return Ok(derive_rule_set(root, paths));
    }
    let p = Path::new(spec);
    let text = std::fs::read_to_string(p)
        .map_err(|e| format!("cannot read rules file {}: {}", p.display(), e))?;
    Ok(rule_set_from(
        parse_rule_file(p, &text)?,
        p.display().to_string(),
    ))
}

/// Auto-derive a taxonomy from the corpus itself: bucket every source file by
/// the first `DERIVE_DEPTH` segments of its path, rank by bucket size, and name
/// the biggest ones.
///
/// This is the whole reason `action=modules` is useful on an unseen codebase.
/// It will not match a hand-curated taxonomy -- buckets are named after
/// directories and the optional `system` column is just the bucket itself --
/// and `.gamedb/modules.txt` exists for when that is not good enough. But it is
/// a real starting point instead of one hardcoded project's rule table.
///
/// Derived from paths rather than from the database on purpose: it has to work
/// on the *first* run of a fresh corpus, when there is no index to derive from.
pub fn derive_rule_set(_root: &Path, paths: &[String]) -> RuleSet {
    let mut bucket: HashMap<String, i64> = HashMap::new();
    for rel in paths {
        let segs: Vec<&str> = rel
            .split(['/', '\\'])
            .filter(|s| !s.is_empty() && *s != "." && Path::new(s).extension().is_none())
            .take(DERIVE_DEPTH)
            .collect();
        if segs.is_empty() {
            continue;
        }
        *bucket.entry(segs.join(".")).or_insert(0) += 1;
    }
    let mut ranked: Vec<(String, i64)> = bucket
        .into_iter()
        .filter(|(_, n)| *n >= DERIVE_MIN_FILES as i64)
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(DERIVE_MAX_MODULES);

    let rows: Vec<(String, String, String, u8)> = ranked
        .into_iter()
        .map(|(ns, _)| {
            let d = ns.split('.').filter(|s| !s.is_empty()).count().clamp(1, 8) as u8;
            (
                format!("n:{ns}"),
                format!("ns.{}", slug(&ns)),
                ns.clone(),
                d,
            )
        })
        .collect();
    let n = rows.len();
    rule_set_from(
        rows,
        format!("derived from the corpus ({n} buckets, depth {DERIVE_DEPTH})"),
    )
}

/// `GameContent` -> `game-content`, `Engine.Rendering` -> `engine.rendering`.
/// Used only to name derived buckets, which is why it can be this crude: a dash
/// goes at a lower-to-upper boundary, and at the last capital of an acronym run
/// that turns into a word (`HTTPClient` -> `http-client`, `UI` -> `ui`).
pub fn slug(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for (i, &c) in cs.iter().enumerate() {
        if c.is_ascii_alphanumeric() {
            let prev_lower = i > 0 && cs[i - 1].is_ascii_lowercase();
            let prev_upper = i > 0 && cs[i - 1].is_ascii_uppercase();
            let next_lower = cs.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            if c.is_ascii_uppercase() && (prev_lower || (prev_upper && next_lower)) {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else if matches!(c, '.' | '/' | '\\' | '_' | '-' | ' ')
            && !out.is_empty()
            && !out.ends_with(['.', '-'])
        {
            out.push('.');
        }
    }
    out.trim_matches(['.', '-']).to_string()
}
