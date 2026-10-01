//! argv, output formatting. No clap: the flag surface is small, fixed, and must
//! match the Python build name for name so the two can be diffed directly.

use crate::db::Val;
use crate::store::*;
use std::path::{Path, PathBuf};

pub const USAGE: &str = concat!(
    "gamedb 1.0.0 - index decompiled source into SQLite (zero dependencies)\n",
    "  gamedb index    -r SRC [-v] [--dry-run] [--force]   incremental; -v N = progress every N\n",
    "  gamedb search   -r SRC QUERY [--limit N]             function-name substring\n",
    "  gamedb strings  -r SRC QUERY [--limit N]             string-literal substring\n",
    "  gamedb read     -r SRC NAME [--out F] [--force]      function body, verbatim\n",
    "  gamedb stats    -r SRC\n",
    "  gamedb modules  -r SRC\n",
    "  gamedb set-module -r SRC --module ID [--state S] [--verified|--unverified]\n",
    "                     [--verified-by WHO] [--remaining TEXT]\n",
    "  gamedb graph    -r SRC NAME [--direction both|callers|callees]\n",
    "  gamedb sql      -r SRC --sql Q [--param V]...        escape hatch: raw SQLite\n",
    "  gamedb selftest\n",
    "global: -r/--root SRC (default .)  --db PATH  --json  -q  -v  -h  -V\n",
    "state:  UNIMPLEMENTED | PARTIALLY_IMPLEMENTED | IMPLEMENTED\n"
);

pub const CMDS: [&str; 10] = [
    "index",
    "search",
    "strings",
    "read",
    "stats",
    "modules",
    "set-module",
    "graph",
    "sql",
    "selftest",
];

#[derive(Default)]
pub struct Args {
    pub cmd: String,
    pub pos: Vec<String>,
    pub root: String,
    pub db: Option<String>,
    pub json: bool,
    pub quiet: bool,
    pub verbose: i64,
    pub limit: usize,
    pub direction: String,
    pub out: Option<String>,
    pub force: bool,
    pub dry_run: bool,
    pub module: Option<String>,
    pub state: Option<String>,
    pub verified: bool,
    pub unverified: bool,
    pub verified_by: Option<String>,
    pub remaining: Option<String>,
    pub sql: Option<String>,
    pub params: Vec<String>,
    pub help: bool,
    pub version: bool,
}

/// Accepts `--flag value` and `--flag=value`. Unknown flags are an error rather
/// than being silently ignored: a typo'd flag that changes nothing is worse.
pub fn parse(argv: Vec<String>) -> Result<Args, String> {
    let mut a = Args {
        root: ".".into(),
        limit: 25,
        direction: "both".into(),
        ..Default::default()
    };
    let mut it = argv.into_iter();
    let first = it.next().unwrap_or_default();
    if first.is_empty() || first == "-h" || first == "--help" {
        a.help = true;
        return Ok(a);
    }
    if first == "-V" || first == "--version" {
        a.version = true;
        return Ok(a);
    }
    if !CMDS.contains(&first.as_str()) {
        return Err(format!(
            "unknown command \"{}\"; expected one of {}",
            first,
            CMDS.join("|")
        ));
    }
    a.cmd = first;
    let rest: Vec<String> = it.collect();
    let mut i = 0;
    while i < rest.len() {
        let tok = rest[i].clone();
        if tok == "--" {
            a.pos.extend(rest[i + 1..].iter().cloned());
            break;
        }
        if !tok.starts_with('-') {
            a.pos.push(tok);
            i += 1;
            continue;
        }
        let (name, _inline) = match tok.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (tok.clone(), None),
        };
        // `--flag=value` never consumes the next token.
        if let Some(v) = _inline {
            match name.as_str() {
                "-r" | "--root" => a.root = v,
                "--db" => a.db = Some(v),
                "-v" | "--verbose" => {
                    a.verbose = v
                        .parse()
                        .map_err(|_| format!("--verbose wants a number, got {:?}", v))?
                }
                "--limit" => {
                    a.limit = v
                        .parse()
                        .map_err(|_| format!("--limit wants a number, got {:?}", v))?
                }
                "--direction" => a.direction = v,
                "--out" => a.out = Some(v),
                "--module" => a.module = Some(v),
                "--state" => a.state = Some(v),
                "--verified-by" => a.verified_by = Some(v),
                "--remaining" => a.remaining = Some(v),
                "--sql" => a.sql = Some(v),
                "--param" => a.params.push(v),
                other => return Err(format!("unknown flag {:?}; try: gamedb --help", other)),
            }
            i += 1;
            continue;
        }
        match name.as_str() {
            "-h" | "--help" => a.help = true,
            "-V" | "--version" => a.version = true,
            "--json" => a.json = true,
            "-q" | "--quiet" => a.quiet = true,
            "--force" => a.force = true,
            "--dry-run" => a.dry_run = true,
            "--verified" => a.verified = true,
            "--unverified" => a.unverified = true,
            "-r" | "--root" => a.root = take(&rest, &mut i, &name)?,
            "--db" => a.db = Some(take(&rest, &mut i, &name)?),
            "-v" | "--verbose" => {
                let v = take(&rest, &mut i, &name)?;
                a.verbose = v
                    .parse()
                    .map_err(|_| format!("--verbose wants a number, got {:?}", v))?;
            }
            "--limit" => {
                let v = take(&rest, &mut i, &name)?;
                a.limit = v
                    .parse()
                    .map_err(|_| format!("--limit wants a number, got {:?}", v))?;
            }
            "--direction" => {
                let v = take(&rest, &mut i, &name)?;
                if !["both", "callers", "callees"].contains(&v.as_str()) {
                    return Err(format!(
                        "--direction must be both|callers|callees, got {:?}",
                        v
                    ));
                }
                a.direction = v;
            }
            "--out" => a.out = Some(take(&rest, &mut i, &name)?),
            "--module" => a.module = Some(take(&rest, &mut i, &name)?),
            "--state" => a.state = Some(take(&rest, &mut i, &name)?),
            "--verified-by" => a.verified_by = Some(take(&rest, &mut i, &name)?),
            "--remaining" => a.remaining = Some(take(&rest, &mut i, &name)?),
            "--sql" => a.sql = Some(take(&rest, &mut i, &name)?),
            "--param" => a.params.push(take(&rest, &mut i, &name)?),
            other => return Err(format!("unknown flag {:?}; try: gamedb --help", other)),
        }
        i += 1;
    }
    Ok(a)
}

fn take(rest: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    if *i + 1 >= rest.len() {
        return Err(format!("{} needs a value", name));
    }
    *i += 1;
    Ok(rest[*i].clone())
}

pub fn jstr(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn jval(v: &Val) -> String {
    match v {
        Val::Null => "null".into(),
        Val::Int(i) => i.to_string(),
        Val::Text(t) => jstr(t),
    }
}

fn emit(args: &Args, json: String, lines: Vec<String>) {
    if args.json {
        println!("{}", json);
    } else if !args.quiet {
        for l in lines {
            println!("{}", l);
        }
    }
}

fn obj(pairs: Vec<(&str, String)>) -> String {
    let body: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{}:{}", jstr(k), v))
        .collect();
    format!("{{{}}}", body.join(","))
}

fn arr(items: Vec<String>) -> String {
    format!("[{}]", items.join(","))
}

pub fn run(a: Args) -> Result<i32, String> {
    let root = PathBuf::from(&a.root);
    if a.cmd != "selftest" && !root.is_dir() {
        return Err(format!("no such directory: {}", a.root));
    }
    let db = a.db.as_deref();
    match a.cmd.as_str() {
        "selftest" => Ok(crate::selftest::run()),

        "index" => {
            let r = index_root(&root, db, a.verbose, a.dry_run, a.force)?;
            let verb = if a.dry_run { "dry-run" } else { "indexed" };
            emit(
                &a,
                obj(vec![
                    ("files", r.files.to_string()),
                    ("functions", r.functions.to_string()),
                    ("strings", r.strings.to_string()),
                    ("symbols", r.symbols.to_string()),
                    ("edges", r.edges.to_string()),
                    ("skipped", r.skipped.to_string()),
                    ("failed", r.failed.to_string()),
                    ("seconds", format!("{:.2}", r.seconds)),
                ]),
                vec![format!(
                    "{} files={} fn={} str={} sym={} edges={} skipped={} failed={} {:.2}s",
                    verb,
                    r.files,
                    r.functions,
                    r.strings,
                    r.symbols,
                    r.edges,
                    r.skipped,
                    r.failed,
                    r.seconds
                )],
            );
            Ok(0)
        }

        "stats" => {
            let s = stats(&root, db);
            if !s.indexed {
                return Err(format!(
                    "no index at {} - run: gamedb index -r {}",
                    crate::db::db_path_for(&root, db).display(),
                    a.root
                ));
            }
            emit(
                &a,
                obj(vec![
                    ("indexed", "true".into()),
                    ("root", jstr(&a.root)),
                    (
                        "db",
                        jstr(&crate::db::db_path_for(&root, db).to_string_lossy()),
                    ),
                    ("files", s.files.to_string()),
                    ("functions", s.functions.to_string()),
                    ("strings", s.strings.to_string()),
                    ("symbols", s.symbols.to_string()),
                    ("edges", s.edges.to_string()),
                ]),
                vec![format!(
                    "files={} functions={} strings={} symbols={} edges={} db={}",
                    s.files,
                    s.functions,
                    s.strings,
                    s.symbols,
                    s.edges,
                    crate::db::db_path_for(&root, db).display()
                )],
            );
            Ok(0)
        }

        "search" | "strings" => {
            let q = a
                .pos
                .first()
                .ok_or_else(|| format!("{} needs a QUERY", a.cmd))?;
            if a.cmd == "search" {
                let rows = search_functions(&root, db, q, a.limit)?;
                emit(
                    &a,
                    arr(rows
                        .iter()
                        .map(|r| {
                            obj(vec![
                                ("name", jstr(r[0].as_str())),
                                ("params", jstr(r[1].as_str())),
                                ("sig", jstr(r[2].as_str())),
                                ("start_line", r[3].as_i64().to_string()),
                                ("end_line", r[4].as_i64().to_string()),
                                ("path", jstr(r[5].as_str())),
                            ])
                        })
                        .collect()),
                    rows.iter()
                        .map(|r| {
                            format!(
                                "{}:{}-{} {}({})",
                                r[5].as_str(),
                                r[3].as_i64(),
                                r[4].as_i64(),
                                r[0].as_str(),
                                r[1].as_str()
                            )
                        })
                        .collect(),
                );
            } else {
                let rows = search_strings(&root, db, q, a.limit)?;
                emit(
                    &a,
                    arr(rows
                        .iter()
                        .map(|r| {
                            obj(vec![
                                ("text", jstr(r[0].as_str())),
                                ("line", r[1].as_i64().to_string()),
                                ("path", jstr(r[2].as_str())),
                            ])
                        })
                        .collect()),
                    rows.iter()
                        .map(|r| format!("{}:{} {}", r[2].as_str(), r[1].as_i64(), r[0].as_str()))
                        .collect(),
                );
            }
            Ok(0)
        }

        "read" => {
            let name = a
                .pos
                .first()
                .ok_or_else(|| "read needs a NAME".to_string())?;
            let found = read_function(&root, db, name)?;
            let Some((sig, path, body)) = found else {
                return Err(format!(
                    "function \"{}\" not indexed - try: gamedb search -r {} {}",
                    name, a.root, name
                ));
            };
            match &a.out {
                Some(p) => {
                    let pb = Path::new(p);
                    if pb.exists() && !a.force {
                        return Err(format!("{} exists - pass --force to overwrite", p));
                    }
                    std::fs::write(pb, &body).map_err(|e| format!("{}: {}", p, e))?;
                    emit(
                        &a,
                        obj(vec![
                            ("sig", jstr(&sig)),
                            ("path", jstr(&path)),
                            ("out", jstr(p)),
                            ("lines", (body.matches('\n').count() + 1).to_string()),
                        ]),
                        vec![format!(
                            "wrote {} ({} lines) from {}",
                            p,
                            body.matches('\n').count() + 1,
                            path
                        )],
                    );
                }
                None => emit(
                    &a,
                    obj(vec![
                        ("sig", jstr(&sig)),
                        ("path", jstr(&path)),
                        ("body", jstr(&body)),
                    ]),
                    body.split('\n').map(str::to_string).collect(),
                ),
            }
            Ok(0)
        }

        "modules" => {
            let rows = list_modules(&root, db)?;
            emit(
                &a,
                arr(rows
                    .iter()
                    .map(|m| {
                        obj(vec![
                            ("module_id", jstr(&m.module_id)),
                            ("system", jstr(&m.system)),
                            ("state", jstr(&m.state)),
                            ("verified", m.verified.to_string()),
                            (
                                "verified_by",
                                jval(
                                    &m.verified_by
                                        .as_ref()
                                        .map_or(Val::Null, |s| Val::Text(s.clone())),
                                ),
                            ),
                            (
                                "verified_at",
                                jval(
                                    &m.verified_at
                                        .as_ref()
                                        .map_or(Val::Null, |s| Val::Text(s.clone())),
                                ),
                            ),
                            ("remaining", jstr(&m.remaining)),
                            ("files", m.files.to_string()),
                            ("functions", m.functions.to_string()),
                        ])
                    })
                    .collect()),
                rows.iter()
                    .map(|m| {
                        format!(
                            "{:<26} {:>5} {:>5}  {:<22} {:<14} {}",
                            m.module_id,
                            m.files,
                            m.functions,
                            m.state,
                            m.verified_by.as_deref().unwrap_or("-"),
                            m.system
                        )
                    })
                    .collect(),
            );
            Ok(0)
        }

        "set-module" => {
            let mid = a
                .module
                .clone()
                .ok_or_else(|| "set-module needs --module ID".to_string())?;
            let patch = ModulePatch {
                state: a.state.clone(),
                verified: if a.verified {
                    Some(true)
                } else if a.unverified {
                    Some(false)
                } else {
                    None
                },
                verified_by: a.verified_by.clone(),
                remaining: a.remaining.clone(),
            };
            let m = set_module_state(&root, db, &mid, &patch)?;
            emit(
                &a,
                obj(vec![
                    ("module_id", jstr(&m.module_id)),
                    ("system", jstr(&m.system)),
                    ("state", jstr(&m.state)),
                    ("verified", m.verified.to_string()),
                    (
                        "verified_by",
                        jval(
                            &m.verified_by
                                .as_ref()
                                .map_or(Val::Null, |s| Val::Text(s.clone())),
                        ),
                    ),
                    (
                        "verified_at",
                        jval(
                            &m.verified_at
                                .as_ref()
                                .map_or(Val::Null, |s| Val::Text(s.clone())),
                        ),
                    ),
                    ("remaining", jstr(&m.remaining)),
                    ("files", m.files.to_string()),
                    ("functions", m.functions.to_string()),
                ]),
                vec![format!(
                    "{} {} verified={} by={}",
                    m.module_id,
                    m.state,
                    m.verified,
                    m.verified_by.as_deref().unwrap_or("-")
                )],
            );
            Ok(0)
        }

        "graph" => {
            let name = a
                .pos
                .first()
                .ok_or_else(|| "graph needs a NAME".to_string())?;
            let rows = graph(&root, db, name, &a.direction, a.limit)?;
            emit(
                &a,
                arr(rows
                    .iter()
                    .map(|r| {
                        obj(vec![
                            ("direction", jstr(r[0].as_str())),
                            ("name", jstr(r[1].as_str())),
                            ("path", jstr(r[2].as_str())),
                            ("call_path", jstr(r[3].as_str())),
                            ("line", r[4].as_i64().to_string()),
                            ("hits", r[5].as_i64().to_string()),
                        ])
                    })
                    .collect()),
                rows.iter()
                    .map(|r| {
                        format!(
                            "{:<6} {:<38} ({}) called at {}:{} (x{})",
                            r[0].as_str(),
                            r[1].as_str(),
                            r[2].as_str(),
                            r[3].as_str(),
                            r[4].as_i64(),
                            r[5].as_i64()
                        )
                    })
                    .collect(),
            );
            Ok(0)
        }

        "sql" => {
            let q = a
                .sql
                .clone()
                .ok_or_else(|| "sql needs --sql QUERY".to_string())?;
            let d = crate::db::Db::open(&crate::db::db_path_for(&root, db), true)?;
            let mut st = d.prepare(&q)?;
            let params: Vec<Val> = a.params.iter().map(|p| Val::Text(p.clone())).collect();
            st.bind_all(&params)?;
            let ncols = st.column_count();
            let names: Vec<String> = (0..ncols).map(|i| st.column_name(i)).collect();
            let mut rows = Vec::new();
            while st.step()? {
                let mut r = Vec::new();
                for i in 0..ncols {
                    r.push(st.column(i));
                }
                rows.push(r);
            }
            let json = arr(rows
                .iter()
                .map(|r| {
                    obj(names
                        .iter()
                        .zip(r.iter())
                        .map(|(k, v)| (k.as_str(), jval(v)))
                        .collect())
                })
                .collect());
            let text = rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|v| v.as_str().to_string())
                        .collect::<Vec<_>>()
                        .join("\t")
                })
                .collect();
            emit(&a, json, text);
            Ok(0)
        }
        _ => Ok(2),
    }
}
