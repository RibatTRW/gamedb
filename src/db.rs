//! SQLite via the C ABI. `winsqlite3.dll` ships with Windows 10 1809+, so the
//! binary links against it directly: no cargo dependency, no vendored C source,
//! no build step, nothing to install. Swap the `#[link]` name for `sqlite3` to
//! build against a normal system SQLite instead.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};

#[allow(non_camel_case_types)]
pub type sqlite3 = c_void;
#[allow(non_camel_case_types)]
pub type sqlite3_stmt = c_void;

const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;
const SQLITE_OPEN_READONLY: c_int = 0x0000_0001;
const SQLITE_NULL: c_int = 5;
const SQLITE_TRANSIENT: isize = -1;

#[link(name = "winsqlite3")]
extern "C" {
    fn sqlite3_open_v2(
        filename: *const c_char,
        db: *mut *mut sqlite3,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close_v2(db: *mut sqlite3) -> c_int;
    fn sqlite3_exec(
        db: *mut sqlite3,
        sql: *const c_char,
        cb: *const c_void,
        arg: *mut c_void,
        err: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut sqlite3,
        sql: *const c_char,
        nbyte: c_int,
        stmt: *mut *mut sqlite3_stmt,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_step(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_finalize(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_reset(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_bind_text(
        stmt: *mut sqlite3_stmt,
        i: c_int,
        v: *const c_char,
        n: c_int,
        d: isize,
    ) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut sqlite3_stmt, i: c_int, v: i64) -> c_int;
    fn sqlite3_bind_null(stmt: *mut sqlite3_stmt, i: c_int) -> c_int;
    fn sqlite3_column_count(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_column_name(stmt: *mut sqlite3_stmt, i: c_int) -> *const c_char;
    fn sqlite3_column_type(stmt: *mut sqlite3_stmt, i: c_int) -> c_int;
    fn sqlite3_column_text(stmt: *mut sqlite3_stmt, i: c_int) -> *const u8;
    fn sqlite3_column_bytes(stmt: *mut sqlite3_stmt, i: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut sqlite3_stmt, i: c_int) -> i64;
    fn sqlite3_last_insert_rowid(db: *mut sqlite3) -> i64;
    fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
    fn sqlite3_busy_timeout(db: *mut sqlite3, ms: c_int) -> c_int;
}

pub type Result<T> = std::result::Result<T, String>;

pub struct Db {
    h: *mut sqlite3,
}

pub struct Stmt<'d> {
    s: *mut sqlite3_stmt,
    db: &'d Db,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Null,
    Int(i64),
    Text(String),
}

impl Val {
    pub fn as_i64(&self) -> i64 {
        match self {
            Val::Int(i) => *i,
            Val::Text(t) => t.parse().unwrap_or(0),
            Val::Null => 0,
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            Val::Text(t) => t,
            _ => "",
        }
    }
}

fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

impl Db {
    pub fn open(path: &Path, readonly: bool) -> Result<Db> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {}", parent.display(), e))?;
            }
        }
        let c = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| "path contains a NUL byte".to_string())?;
        let flags = if readonly {
            SQLITE_OPEN_READONLY
        } else {
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE
        };
        let mut h: *mut sqlite3 = std::ptr::null_mut();
        let rc = unsafe { sqlite3_open_v2(c.as_ptr(), &mut h, flags, std::ptr::null()) };
        if rc != SQLITE_OK {
            let msg = if h.is_null() {
                format!("open failed rc={}", rc)
            } else {
                cstr(unsafe { sqlite3_errmsg(h) })
            };
            if !h.is_null() {
                unsafe { sqlite3_close_v2(h) };
            }
            return Err(msg);
        }
        let db = Db { h };
        unsafe { sqlite3_busy_timeout(db.h, 10_000) };
        if !readonly {
            db.exec("PRAGMA journal_mode=WAL")?;
        }
        Ok(db)
    }

    pub fn exec(&self, sql: &str) -> Result<()> {
        let c = CString::new(sql).map_err(|_| "sql contains a NUL byte".to_string())?;
        let mut err: *mut c_char = std::ptr::null_mut();
        let rc = unsafe {
            sqlite3_exec(
                self.h,
                c.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                &mut err,
            )
        };
        if rc != SQLITE_OK {
            let msg = cstr(err);
            if !err.is_null() {
                unsafe { sqlite3_free(err as *mut c_void) };
            }
            return Err(if msg.is_empty() {
                format!("exec failed rc={}", rc)
            } else {
                msg
            });
        }
        Ok(())
    }

    pub fn prepare(&self, sql: &str) -> Result<Stmt<'_>> {
        let c = CString::new(sql).map_err(|_| "sql contains a NUL byte".to_string())?;
        let mut s: *mut sqlite3_stmt = std::ptr::null_mut();
        let rc =
            unsafe { sqlite3_prepare_v2(self.h, c.as_ptr(), -1, &mut s, std::ptr::null_mut()) };
        if rc != SQLITE_OK || s.is_null() {
            return Err(cstr(unsafe { sqlite3_errmsg(self.h) }));
        }
        Ok(Stmt { s, db: self })
    }

    /// Run a statement that returns no rows.
    pub fn run(&self, sql: &str, params: &[Val]) -> Result<()> {
        let mut st = self.prepare(sql)?;
        st.bind_all(params)?;
        while st.step()? {}
        Ok(())
    }

    /// Run a query and collect every row.
    pub fn query(&self, sql: &str, params: &[Val]) -> Result<Vec<Vec<Val>>> {
        let mut st = self.prepare(sql)?;
        st.bind_all(params)?;
        let n = st.column_count();
        let mut rows = Vec::new();
        while st.step()? {
            let mut r = Vec::with_capacity(n);
            for i in 0..n {
                r.push(st.column(i));
            }
            rows.push(r);
        }
        Ok(rows)
    }

    pub fn query_one(&self, sql: &str, params: &[Val]) -> Result<Option<Vec<Val>>> {
        Ok(self.query(sql, params)?.into_iter().next())
    }

    pub fn scalar_i64(&self, sql: &str, params: &[Val]) -> Result<i64> {
        Ok(self
            .query_one(sql, params)?
            .map(|r| r.first().map_or(0, |v| v.as_i64()))
            .unwrap_or(0))
    }

    pub fn last_insert_rowid(&self) -> i64 {
        unsafe { sqlite3_last_insert_rowid(self.h) }
    }

    pub fn table_exists(&self, name: &str) -> Result<bool> {
        Ok(self.scalar_i64(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?",
            &[Val::Text(name.into())],
        )? > 0)
    }

    pub fn column_exists(&self, table: &str, col: &str) -> Result<bool> {
        if !self.table_exists(table)? {
            return Ok(false);
        }
        Ok(self.scalar_i64(
            "SELECT count(*) FROM pragma_table_info(?) WHERE name=?",
            &[Val::Text(table.into()), Val::Text(col.into())],
        )? > 0)
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        if !self.h.is_null() {
            unsafe { sqlite3_close_v2(self.h) };
            self.h = std::ptr::null_mut();
        }
    }
}

impl<'d> Stmt<'d> {
    pub fn bind_all(&mut self, params: &[Val]) -> Result<()> {
        // A statement that has run to SQLITE_DONE must be reset before it can be
        // rebound; without this, the edge loop's second INSERT is SQLITE_MISUSE.
        unsafe { sqlite3_reset(self.s) };
        for (i, p) in params.iter().enumerate() {
            let idx = (i + 1) as c_int;
            let rc = match p {
                Val::Null => unsafe { sqlite3_bind_null(self.s, idx) },
                Val::Int(v) => unsafe { sqlite3_bind_int64(self.s, idx, *v) },
                Val::Text(t) => {
                    let bytes = t.as_bytes();
                    unsafe {
                        sqlite3_bind_text(
                            self.s,
                            idx,
                            bytes.as_ptr() as *const c_char,
                            bytes.len() as c_int,
                            SQLITE_TRANSIENT,
                        )
                    }
                }
            };
            if rc != SQLITE_OK {
                return Err(cstr(unsafe { sqlite3_errmsg(self.db.h) }));
            }
        }
        Ok(())
    }

    /// Advance the cursor. `Ok(true)` when a row is available.
    pub fn step(&mut self) -> Result<bool> {
        match unsafe { sqlite3_step(self.s) } {
            SQLITE_ROW => Ok(true),
            SQLITE_DONE => Ok(false),
            _ => Err(cstr(unsafe { sqlite3_errmsg(self.db.h) })),
        }
    }

    pub fn column_count(&self) -> usize {
        unsafe { sqlite3_column_count(self.s) as usize }
    }

    pub fn column_name(&self, i: usize) -> String {
        cstr(unsafe { sqlite3_column_name(self.s, i as c_int) })
    }

    pub fn column(&self, i: usize) -> Val {
        let ct = unsafe { sqlite3_column_type(self.s, i as c_int) };
        if ct == SQLITE_NULL {
            return Val::Null;
        }
        if ct == 1 || ct == 2 {
            return Val::Int(unsafe { sqlite3_column_int64(self.s, i as c_int) });
        }
        let p = unsafe { sqlite3_column_text(self.s, i as c_int) };
        if p.is_null() {
            return Val::Null;
        }
        let n = unsafe { sqlite3_column_bytes(self.s, i as c_int) } as usize;
        let slice = unsafe { std::slice::from_raw_parts(p, n) };
        Val::Text(String::from_utf8_lossy(slice).into_owned())
    }
}

impl Drop for Stmt<'_> {
    fn drop(&mut self) {
        if !self.s.is_null() {
            unsafe { sqlite3_finalize(self.s) };
            self.s = std::ptr::null_mut();
        }
    }
}

extern "C" {
    fn sqlite3_free(p: *mut c_void);
}

/// Schema text is tab-indented to match the TypeScript original byte for byte,
/// so a strict `sqlite_master` dump diff between the two builds is free.
pub const SCHEMA: &str = r#"
		CREATE TABLE IF NOT EXISTS files(
			id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL,
			mtime INTEGER NOT NULL, size INTEGER NOT NULL,
			edges_mtime INTEGER NOT NULL DEFAULT 0);
		CREATE TABLE IF NOT EXISTS functions(
			id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL,
			name TEXT NOT NULL, params TEXT, sig TEXT,
			start_line INTEGER, end_line INTEGER);
		CREATE INDEX IF NOT EXISTS functions_name ON functions(name);
		CREATE TABLE IF NOT EXISTS strings(
			id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL,
			line INTEGER, text TEXT NOT NULL);
		CREATE INDEX IF NOT EXISTS strings_text ON strings(text);
		CREATE TABLE IF NOT EXISTS symbols(
			id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL,
			kind TEXT NOT NULL, name TEXT NOT NULL, line INTEGER);
		CREATE INDEX IF NOT EXISTS symbols_name ON symbols(name);
		CREATE INDEX IF NOT EXISTS symbols_file ON symbols(file_id);
		CREATE TABLE IF NOT EXISTS edges(
			src_id INTEGER NOT NULL, dst_id INTEGER NOT NULL,
			line INTEGER, hits INTEGER NOT NULL DEFAULT 1,
			PRIMARY KEY(src_id, dst_id)) WITHOUT ROWID;
		CREATE INDEX IF NOT EXISTS edges_dst ON edges(dst_id);
		CREATE TABLE IF NOT EXISTS module_status(
			module_id TEXT PRIMARY KEY,
			system TEXT,
			state TEXT NOT NULL DEFAULT 'UNIMPLEMENTED'
				CHECK(state IN ('UNIMPLEMENTED','PARTIALLY_IMPLEMENTED','IMPLEMENTED')),
			verified INTEGER NOT NULL DEFAULT 0,
			verified_by TEXT, verified_at TEXT, remaining TEXT);
"#;

pub fn db_path_for(root: &Path, db_override: Option<&str>) -> PathBuf {
    match db_override {
        Some(p) => PathBuf::from(p),
        None => root.join(".gamedb").join("index.sqlite"),
    }
}

/// Additive migrations for databases written by an older build.
pub fn migrate(db: &Db) -> Result<()> {
    for (ddl, col, tbl) in [
        (
            "ALTER TABLE files ADD COLUMN edges_mtime INTEGER NOT NULL DEFAULT 0",
            "edges_mtime",
            "files",
        ),
        (
            "ALTER TABLE functions ADD COLUMN sym_id INTEGER",
            "sym_id",
            "functions",
        ),
        (
            "ALTER TABLE files ADD COLUMN module TEXT",
            "module",
            "files",
        ),
    ] {
        if !db.column_exists(tbl, col)? {
            db.exec(ddl)?;
        }
    }
    Ok(())
}

pub fn open_schema(root: &Path, db_override: Option<&str>, create: bool) -> Result<Db> {
    if create {
        let db = Db::open(&db_path_for(root, db_override), false)?;
        db.exec(SCHEMA)?;
        migrate(&db)?;
        return Ok(db);
    }
    open_existing(root, db_override, true)
}

/// Open an index that must already exist. `readonly` is the default for query
/// commands; writers pass `false` so SQLite does not refuse the UPDATE.
pub fn open_existing(root: &Path, db_override: Option<&str>, readonly: bool) -> Result<Db> {
    let path = db_path_for(root, db_override);
    if !path.exists() {
        return Err(format!(
            "no index at {} - run: gamedb index -r {}",
            path.display(),
            root.display()
        ));
    }
    Db::open(&path, readonly)
}
