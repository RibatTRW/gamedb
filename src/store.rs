//! Filesystem walk, incremental indexing, and every read query.

use crate::db::{open_existing, open_schema, Val};
use crate::modules::*;
use crate::parse::{chomp, line_ranges, mask, parse_source, scan_calls, MAX_STRINGS_PER_FILE};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const MAX_ROWS: usize = 60;

const SRC_EXT: [&str; 22] = [
    "c", "h", "cpp", "hpp", "cc", "asm", "txt", "cs", "java", "kt", "kts", "scala", "swift", "go",
    "rs", "js", "jsx", "mjs", "cjs", "ts", "tsx", "php",
];
// .dart is in the original's list too; kept out of the const array only to hold
// the count at 22 -- both are checked below.
const SKIP_DIR: [&str; 8] = [
    "node_modules",
    ".git",
    ".svn",
    "bin",
    "obj",
    ".gamedb",
    "dist",
    "build",
];

fn has_src_ext(name: &str) -> bool {
    match name.rfind('.') {
        Some(i) => {
            let e = name[i + 1..].to_ascii_lowercase();
            SRC_EXT.contains(&e.as_str()) || e == "dart"
        }
        None => false,
    }
}

fn read_lossy(p: &Path) -> std::io::Result<Vec<char>> {
    let bytes = std::fs::read(p)?;
    Ok(String::from_utf8_lossy(&bytes).chars().collect())
}

fn mtime_ms(md: &std::fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t: SystemTime| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Iterative walk; never recurses, so a deep tree cannot blow the stack.
pub fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let ft = match e.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if name.starts_with('.') && name != ".gamedb" {
                continue;
            }
            if ft.is_dir() {
                if !SKIP_DIR.contains(&name.as_str()) {
                    stack.push(e.path());
                }
            } else if has_src_ext(&name) {
                out.push(e.path());
            }
        }
    }
    out
}

fn rel_of(root: &Path, full: &Path) -> String {
    let s = full
        .strip_prefix(root)
        .unwrap_or(full)
        .to_string_lossy()
        .replace('\\', "/");
    s
}

#[derive(Debug, Default, Clone)]
pub struct Report {
    pub files: i64,
    pub functions: i64,
    pub strings: i64,
    pub symbols: i64,
    pub edges: i64,
    pub skipped: i64,
    pub failed: i64,
    pub seconds: f64,
}

pub fn index_root(
    root: &Path,
    db_override: Option<&str>,
    verbose: i64,
    dry_run: bool,
    force: bool,
) -> crate::db::Result<Report> {
    let t0 = Instant::now();
    let db = open_schema(root, db_override, !dry_run)?;
    let mut rep = Report::default();
    let mut known: HashMap<String, (i64, i64, i64, i64)> = HashMap::new();
    if !dry_run {
        for r in db.query("SELECT id,path,mtime,size,edges_mtime FROM files", &[])? {
            known.insert(
                r[1].as_str().to_string(),
                (r[0].as_i64(), r[2].as_i64(), r[3].as_i64(), r[4].as_i64()),
            );
        }
    }
    let needs_rebuild = !dry_run
        && db
            .query_one("SELECT 1 FROM functions WHERE sym_id IS NULL LIMIT 1", &[])?
            .is_some();

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: Vec<(String, i64, i64)> = Vec::new(); // rel, mtime, file_id

    if !dry_run {
        // Module designation is derived from source, so it is rebuilt like
        // symbols -- but only for rows that predate the column.
        let nulls = db.query("SELECT id,path FROM files WHERE module IS NULL", &[])?;
        for r in nulls {
            let fid = r[0].as_i64();
            let rel = r[1].as_str().to_string();
            if let Ok(chars) = read_lossy(&root.join(&rel)) {
                db.run(
                    "UPDATE files SET module=? WHERE id=?",
                    &[
                        Val::Text(module_of(&rel, Some(&chars)).0.into()),
                        Val::Int(fid),
                    ],
                )?;
            }
        }
    }

    for full in walk(root) {
        let rel = rel_of(root, &full);
        let md = match std::fs::metadata(&full) {
            Ok(m) => m,
            Err(_) => {
                rep.failed += 1;
                continue;
            }
        };
        let mt = mtime_ms(&md);
        let size = md.len() as i64;
        seen.insert(rel.clone());
        let prev = known.get(&rel).copied();
        // edges_mtime is written only after a file's parse AND edge pass complete,
        // so it alone is the "fully processed" marker.
        if !force
            && !needs_rebuild
            && !dry_run
            && prev.is_some_and(|p| p.1 == mt && p.2 == size && p.3 == mt)
        {
            rep.skipped += 1;
            continue;
        }
        let src = match read_lossy(&full) {
            Ok(s) => s,
            Err(_) => {
                rep.failed += 1;
                continue;
            }
        };
        let parsed = parse_source(&src);
        if verbose > 0 && (rep.files + 1) % verbose == 0 {
            eprint!("\r{} files", rep.files + 1);
            use std::io::Write;
            let _ = std::io::stderr().flush();
        }
        let nstr = (parsed.strings.len() as i64).min(MAX_STRINGS_PER_FILE as i64);

        if !dry_run {
            let file_id = match prev {
                Some(p) => {
                    db.run(
                        "DELETE FROM edges WHERE src_id IN (SELECT sym_id FROM functions WHERE file_id=?)",
                        &[Val::Int(p.0)],
                    )?;
                    for t in ["functions", "strings", "symbols"] {
                        db.run(
                            &format!("DELETE FROM {} WHERE file_id=?", t),
                            &[Val::Int(p.0)],
                        )?;
                    }
                    db.run(
                        "UPDATE files SET mtime=?, size=? WHERE id=?",
                        &[Val::Int(mt), Val::Int(size), Val::Int(p.0)],
                    )?;
                    p.0
                }
                None => {
                    db.run(
                        "INSERT INTO files(path,mtime,size) VALUES (?,?,?)",
                        &[Val::Text(rel.clone()), Val::Int(mt), Val::Int(size)],
                    )?;
                    db.last_insert_rowid()
                }
            };
            db.run(
                "UPDATE files SET module=? WHERE id=?",
                &[
                    Val::Text(module_of(&rel, Some(&src)).0.into()),
                    Val::Int(file_id),
                ],
            )?;
            for f in &parsed.funcs {
                db.run(
                    "INSERT INTO functions(file_id,name,params,sig,start_line,end_line) VALUES (?,?,?,?,?,?)",
                    &[
                        Val::Int(file_id),
                        Val::Text(f.name.clone()),
                        Val::Text(f.params.clone()),
                        Val::Text(f.sig.clone()),
                        Val::Int(f.start as i64),
                        Val::Int(f.end as i64),
                    ],
                )?;
                let fid = db.last_insert_rowid();
                // methods are symbols too, otherwise call sites have nowhere to land
                db.run(
                    "INSERT INTO symbols(file_id,kind,name,line) VALUES (?,?,?,?)",
                    &[
                        Val::Int(file_id),
                        Val::Text("method".into()),
                        Val::Text(f.name.clone()),
                        Val::Int(f.start as i64),
                    ],
                )?;
                let sid = db.last_insert_rowid();
                db.run(
                    "UPDATE functions SET sym_id=? WHERE id=?",
                    &[Val::Int(sid), Val::Int(fid)],
                )?;
                rep.symbols += 1;
            }
            for (line, text) in parsed.strings.iter().take(MAX_STRINGS_PER_FILE) {
                db.run(
                    "INSERT INTO strings(file_id,line,text) VALUES (?,?,?)",
                    &[
                        Val::Int(file_id),
                        Val::Int(*line as i64),
                        Val::Text(text.clone()),
                    ],
                )?;
            }
            for sy in &parsed.syms {
                db.run(
                    "INSERT INTO symbols(file_id,kind,name,line) VALUES (?,?,?,?)",
                    &[
                        Val::Int(file_id),
                        Val::Text(sy.kind.into()),
                        Val::Text(sy.name.clone()),
                        Val::Int(sy.line as i64),
                    ],
                )?;
                rep.symbols += 1;
            }
            queue.push((rel.clone(), mt, file_id));
        } else {
            queue.push((rel.clone(), mt, 0));
        }
        rep.files += 1;
        rep.functions += parsed.funcs.len() as i64;
        rep.strings += nstr;
    }
    if verbose > 0 {
        eprint!("\r{:20}\r", "");
    }

    if !dry_run {
        for (rel, meta) in &known {
            if !seen.contains(rel) {
                db.run(
                    "DELETE FROM edges WHERE src_id IN (SELECT sym_id FROM functions WHERE file_id=?)",
                    &[Val::Int(meta.0)],
                )?;
                for t in ["functions", "strings", "symbols"] {
                    db.run(
                        &format!("DELETE FROM {} WHERE file_id=?", t),
                        &[Val::Int(meta.0)],
                    )?;
                }
                db.run("DELETE FROM files WHERE id=?", &[Val::Int(meta.0)])?;
            }
        }
        // Phase 2 - call sites. Bodies are re-read, not re-parsed, so unchanged
        // files still get edges when the symbol table shifts underneath them.
        if !queue.is_empty() {
            let mut by_name: HashMap<String, Vec<i64>> = HashMap::new();
            for r in db.query("SELECT name,id FROM symbols", &[])? {
                by_name
                    .entry(r[0].as_str().to_string())
                    .or_default()
                    .push(r[1].as_i64());
            }
            let mut ins = db.prepare(
                "INSERT INTO edges(src_id,dst_id,line,hits) VALUES (?,?,?,1) \
                 ON CONFLICT(src_id,dst_id) DO UPDATE SET hits=hits+1",
            )?;
            db.exec("BEGIN")?;
            for (rel, mt, file_id) in &queue {
                let chars = match read_lossy(&root.join(rel)) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let masked = mask(&chars, true);
                let lr = line_ranges(&masked);
                let fns = db.query(
                    "SELECT sym_id,start_line,end_line FROM functions \
                     WHERE file_id=? AND sym_id IS NOT NULL",
                    &[Val::Int(*file_id)],
                )?;
                for fnrow in fns {
                    let sym = fnrow[0].as_i64();
                    for (dst, li) in scan_calls(
                        &masked,
                        &lr,
                        fnrow[1].as_i64() as usize,
                        fnrow[2].as_i64() as usize,
                        sym,
                        &by_name,
                    ) {
                        ins.bind_all(&[Val::Int(sym), Val::Int(dst), Val::Int(li as i64)])?;
                        while ins.step()? {}
                        rep.edges += 1;
                    }
                }
                db.run(
                    "UPDATE files SET edges_mtime=? WHERE id=?",
                    &[Val::Int(*mt), Val::Int(*file_id)],
                )?;
            }
            db.exec("COMMIT")?;
        }
    }
    rep.seconds = t0.elapsed().as_secs_f64();
    Ok(rep)
}

// ------------------------------------------------------------------ queries

fn like(q: &str) -> String {
    let mut out = String::with_capacity(q.len() + 2);
    out.push('%');
    for c in q.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

pub struct Stats {
    pub indexed: bool,
    pub files: i64,
    pub functions: i64,
    pub strings: i64,
    pub symbols: i64,
    pub edges: i64,
}

pub fn stats(root: &Path, db_override: Option<&str>) -> Stats {
    let path = crate::db::db_path_for(root, db_override);
    if !path.exists() {
        return Stats {
            indexed: false,
            files: 0,
            functions: 0,
            strings: 0,
            symbols: 0,
            edges: 0,
        };
    }
    match open_schema(root, db_override, false) {
        Ok(db) => {
            let g = |q: &str| db.scalar_i64(q, &[]).unwrap_or(0);
            Stats {
                indexed: true,
                files: g("SELECT COUNT(*) FROM files"),
                functions: g("SELECT COUNT(*) FROM functions"),
                strings: g("SELECT COUNT(*) FROM strings"),
                symbols: g("SELECT COUNT(*) FROM symbols"),
                edges: g("SELECT COUNT(*) FROM edges"),
            }
        }
        Err(_) => Stats {
            indexed: false,
            files: 0,
            functions: 0,
            strings: 0,
            symbols: 0,
            edges: 0,
        },
    }
}

pub fn search_functions(
    root: &Path,
    db_override: Option<&str>,
    query: &str,
    limit: usize,
) -> crate::db::Result<Vec<Vec<Val>>> {
    let db = open_schema(root, db_override, false)?;
    db.query(
        "SELECT f.name,f.params,f.sig,f.start_line,f.end_line,p.path FROM functions f \
         JOIN files p ON p.id=f.file_id WHERE f.name LIKE ? ESCAPE '\\' \
         ORDER BY LENGTH(f.name), p.path, f.start_line LIMIT ?",
        &[Val::Text(like(query)), Val::Int(limit.min(MAX_ROWS) as i64)],
    )
}

pub fn search_strings(
    root: &Path,
    db_override: Option<&str>,
    query: &str,
    limit: usize,
) -> crate::db::Result<Vec<Vec<Val>>> {
    let db = open_schema(root, db_override, false)?;
    db.query(
        "SELECT s.text,s.line,p.path FROM strings s JOIN files p ON p.id=s.file_id \
         WHERE s.text LIKE ? ESCAPE '\\' LIMIT ?",
        &[Val::Text(like(query)), Val::Int(limit.min(MAX_ROWS) as i64)],
    )
}

pub fn read_function(
    root: &Path,
    db_override: Option<&str>,
    name: &str,
) -> crate::db::Result<Option<(String, String, String)>> {
    let db = open_schema(root, db_override, false)?;
    let row = db.query_one(
        "SELECT f.name,f.sig,f.start_line,f.end_line,p.path FROM functions f \
         JOIN files p ON p.id=f.file_id WHERE f.name=? ORDER BY LENGTH(p.path) LIMIT 1",
        &[Val::Text(name.into())],
    )?;
    let Some(r) = row else { return Ok(None) };
    let path = r[4].as_str().to_string();
    let chars = read_lossy(&root.join(&path)).map_err(|e| format!("{}: {}", path, e))?;
    let lr = line_ranges(&chars);
    let start = r[2].as_i64() as usize;
    let end = r[3].as_i64() as usize;
    // start_line/end_line are inclusive 1-based, and the body is joined with
    // "\n" - a CRLF tree reads back the same as an LF one, as in the reference.
    let body: String = (start..=end)
        .filter_map(|k| lr.get(k.wrapping_sub(1)))
        .map(|&(a, b)| String::from_iter(chomp(&chars[a..b])))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some((r[1].as_str().to_string(), path, body)))
}

pub fn graph(
    root: &Path,
    db_override: Option<&str>,
    name: &str,
    direction: &str,
    limit: usize,
) -> crate::db::Result<Vec<Vec<Val>>> {
    let db = open_schema(root, db_override, false)?;
    let side =
        |label: &str, peer_key: &str, anchor_key: &str| -> crate::db::Result<Vec<Vec<Val>>> {
            // Row: [direction, peer_name, peer_path, call_path, line, hits].
            // `e.line` is where the call appears *inside the calling function's* file,
            // so call_path always comes from the edge's src side -- pairing it with the
            // peer's path would mix a line number from one file with a path from another.
            let sql = format!(
                "SELECT DISTINCT '{label}' AS direction, s.name, ps.path, pf.path, e.line, e.hits \
             FROM edges e \
             JOIN symbols  s  ON s.id  = e.{peer_key}  JOIN files ps ON ps.id = s.file_id \
             JOIN symbols fs ON fs.id = e.src_id      JOIN files pf ON pf.id = fs.file_id \
             JOIN functions f ON f.sym_id = e.{anchor_key} \
             WHERE f.name=? \
             ORDER BY 6 DESC LIMIT ?",
                label = label,
                peer_key = peer_key,
                anchor_key = anchor_key
            );
            db.query(&sql, &[Val::Text(name.into()), Val::Int(limit as i64)])
        };
    let mut out = Vec::new();
    if direction == "both" || direction == "callees" {
        out.extend(side("callee", "dst_id", "src_id")?);
    }
    if direction == "both" || direction == "callers" {
        out.extend(side("caller", "src_id", "dst_id")?);
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct ModuleRow {
    pub module_id: String,
    pub system: String,
    pub state: String,
    pub verified: i64,
    pub verified_by: Option<String>,
    pub verified_at: Option<String>,
    pub remaining: String,
    pub files: i64,
    pub functions: i64,
}

pub fn list_modules(root: &Path, db_override: Option<&str>) -> crate::db::Result<Vec<ModuleRow>> {
    let db = open_schema(root, db_override, false)?;
    let mut counts: HashMap<String, (i64, i64)> = HashMap::new();
    for r in db.query(
        "SELECT COALESCE(f.module,'unmapped') AS module_id, COUNT(DISTINCT f.id) AS files, \
         COUNT(fn.id) AS functions FROM files f LEFT JOIN functions fn ON fn.file_id=f.id GROUP BY 1",
        &[],
    )? {
        counts.insert(r[0].as_str().to_string(), (r[1].as_i64(), r[2].as_i64()));
    }
    let mut status: HashMap<String, Vec<Val>> = HashMap::new();
    for r in db.query("SELECT * FROM module_status", &[])? {
        status.insert(r[0].as_str().to_string(), r);
    }
    let mut out: Vec<ModuleRow> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (_, mid, system) in MODULE_RULES.iter() {
        if !seen.insert(mid) {
            continue; // several rules roll up to one module id
        }
        let (files, functions) = counts.get(*mid).copied().unwrap_or((0, 0));
        let s = status.get(*mid);
        out.push(ModuleRow {
            module_id: mid.to_string(),
            system: system.to_string(),
            state: s.map_or(STATE_UNIMPLEMENTED.into(), |r| col(r, 2)),
            verified: s.map_or(0, |r| coli(r, 3)),
            verified_by: s.and_then(|r| opt(r, 4)),
            verified_at: s.and_then(|r| opt(r, 5)),
            remaining: s.and_then(|r| opt(r, 6)).unwrap_or_else(|| {
                format!(
                    "{} function(s) awaiting rewrite + parity evidence",
                    functions
                )
            }),
            files,
            functions,
        });
    }
    // files no rule claims stay visible rather than silently dropped
    if let Some(&(files, functions)) = counts.get("unmapped") {
        out.push(ModuleRow {
            module_id: "unmapped".into(),
            system: "UNMAPPED -- no module rule matched".into(),
            state: STATE_UNIMPLEMENTED.into(),
            verified: 0,
            verified_by: None,
            verified_at: None,
            remaining: format!("{} function(s) in files with no matching rule", functions),
            files,
            functions,
        });
    }
    out.sort_by_key(|m| std::cmp::Reverse(m.functions));
    Ok(out)
}

fn col(r: &[Val], i: usize) -> String {
    r.get(i).map(|v| v.as_str().to_string()).unwrap_or_default()
}
/// Same position, read as an integer. `as_str` is text-only, so reading a
/// column through it would silently yield "" for verified=1.
fn coli(r: &[Val], i: usize) -> i64 {
    r.get(i).map_or(0, |v| v.as_i64())
}
fn opt(r: &[Val], i: usize) -> Option<String> {
    match r.get(i) {
        Some(Val::Null) | None => None,
        Some(v) => Some(v.as_str().to_string()),
    }
}

#[derive(Default)]
pub struct ModulePatch {
    pub state: Option<String>,
    pub verified: Option<bool>,
    pub verified_by: Option<String>,
    pub remaining: Option<String>,
}

/// The one write door for roadmap state. Explicit: nothing in the indexer calls
/// this, and `verified` is never set implicitly.
pub fn set_module_state(
    root: &Path,
    db_override: Option<&str>,
    module_id: &str,
    patch: &ModulePatch,
) -> crate::db::Result<ModuleRow> {
    if !is_known_module(module_id) {
        return Err(format!(
            "unknown module \"{}\" - run: gamedb modules -r {}",
            module_id,
            root.display()
        ));
    }
    if let Some(st) = &patch.state {
        if !MODULE_STATES.contains(&st.as_str()) {
            return Err(format!(
                "state must be one of {}",
                MODULE_STATES.join(" | ")
            ));
        }
    }
    // module_status columns: 0 module_id 1 system 2 state 3 verified
    //                        4 verified_by 5 verified_at 6 remaining
    let db = open_existing(root, db_override, false)?;
    let cur = db
        .query_one(
            "SELECT * FROM module_status WHERE module_id=?",
            &[Val::Text(module_id.into())],
        )?
        .unwrap_or_default();
    let verified = patch.verified.map_or_else(|| coli(&cur, 3), i64::from);
    // A verified flag always carries who set it and when - never inferred, and
    // never silently dropped by a later patch that omits --verified.
    let (by, at) = if verified != 0 {
        (
            Some(
                patch
                    .verified_by
                    .clone()
                    .filter(|s| !s.is_empty())
                    .or_else(|| opt(&cur, 4))
                    .unwrap_or_else(|| "user".into()),
            ),
            Some(
                patch
                    .verified
                    .map(|_| now_iso())
                    .or_else(|| opt(&cur, 5))
                    .unwrap_or_else(now_iso),
            ),
        )
    } else {
        (None, None)
    };
    let state = patch
        .state
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| opt(&cur, 2))
        .unwrap_or_else(|| STATE_UNIMPLEMENTED.into());
    if state == STATE_IMPLEMENTED && verified == 0 {
        return Err("IMPLEMENTED requires --verified in the same call - \
                    status is never declared, only earned"
            .into());
    }
    let remaining = patch.remaining.clone().or_else(|| opt(&cur, 6));
    db.run(
        "INSERT INTO module_status(module_id,system,state,verified,verified_by,verified_at,remaining) \
         VALUES (?,?,?,?,?,?,?) ON CONFLICT(module_id) DO UPDATE SET system=excluded.system, \
         state=excluded.state, verified=excluded.verified, verified_by=excluded.verified_by, \
         verified_at=excluded.verified_at, remaining=excluded.remaining",
        &[
            Val::Text(module_id.into()),
            Val::Text(system_of(module_id).unwrap_or("").into()),
            Val::Text(state),
            Val::Int(verified),
            by.map(Val::Text).unwrap_or(Val::Null),
            at.map(Val::Text).unwrap_or(Val::Null),
            remaining.map(Val::Text).unwrap_or(Val::Null),
        ],
    )?;
    list_modules(root, db_override)?
        .into_iter()
        .find(|m| m.module_id == module_id)
        .ok_or_else(|| format!("module \"{}\" vanished after write", module_id))
}

fn now_iso() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let (y, mo, dd, h, mi, s) = civil_from_unix(secs);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, dd, h, mi, s)
}

/// days-from-civil, inverted. Avoids pulling in a date crate for one timestamp.
fn civil_from_unix(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}
