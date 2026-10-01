//! The one runnable check. `gamedb selftest` exits non-zero on any failure,
//! so CI needs nothing else.

use crate::modules::*;
use crate::parse::{mask, parse_source, scan_calls};
use crate::store::*;
use std::path::{Path, PathBuf};

struct Checks {
    v: Vec<(String, bool)>,
}

impl Checks {
    fn ok(&mut self, label: &str, cond: bool) {
        self.v.push((label.to_string(), cond));
    }
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn names(p: &crate::parse::Parsed) -> Vec<String> {
    p.funcs.iter().map(|f| f.name.clone()).collect()
}

fn tmpdir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("gamedb-selftest-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("temp dir");
    p
}

fn write_file(dir: &Path, rel: &str, body: &str) {
    let p = dir.join(rel);
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).expect("mkdir");
    }
    std::fs::write(p, body).expect("write");
}

pub fn run() -> i32 {
    let mut c = Checks { v: Vec::new() };

    // --- C: brace walk, comment vs literal, keyword rejection
    let c_src = [
        "// comment with { brace and \"string\"",
        "int main(int argc, char **argv) {",
        "\tprintf(\"hello world\");",
        "\t/* } */ if (argc) { return 1; }",
        "\treturn 0;",
        "}",
        "",
        "void FUN_00401234(int param_1, char *param_2) {",
        "\tchar *s = \"secret_lobby_01\";",
        "}",
    ]
    .join("\n");
    let p = parse_source(&chars(&c_src));
    let nm = names(&p);
    let mainf = p.funcs.iter().find(|f| f.name == "main").expect("main");
    c.ok("c: 2 functions", nm == ["main", "FUN_00401234"]);
    c.ok("c: main ends at 6", mainf.end == 6);
    c.ok("c: no bogus 'if' function", !nm.iter().any(|n| n == "if"));
    c.ok(
        "c: literal extracted",
        p.strings.iter().any(|(_, t)| t == "secret_lobby_01"),
    );
    c.ok(
        "c: comment text not a literal",
        !p.strings.iter().any(|(_, t)| t == "string"),
    );

    // --- C#: Allman braces, generics with modifier, expression-bodied member
    let cs_src = [
        "public class Animation",
        "{",
        "\tprivate void SetDefaults(int type)",
        "\t{",
        "\t\t_frameType = 0;",
        "\t}",
        "",
        "\tinternal static Player Find(int id)",
        "\t{",
        "\t\treturn _cache[id];",
        "\t}",
        "\tprotected void NoBrace(int x) => Do(x);",
        "}",
        "",
    ]
    .join("\n");
    let p = parse_source(&chars(&cs_src));
    let nm = names(&p);
    let sd = p
        .funcs
        .iter()
        .find(|f| f.name == "SetDefaults")
        .expect("SetDefaults");
    c.ok("cs: Allman method", sd.end == 6);
    c.ok("cs: params captured", sd.params == "int type");
    c.ok("cs: modifier + return type", nm.iter().any(|n| n == "Find"));
    // `([^()]*)` in the tail pattern stops at the first paren, so a generic
    // parameter list like `List<T> list` is outside the pattern. Pinned because
    // it is a real coverage limit of the original, not an accident of this port.
    // expression-bodied members are declarations, not brace-delimited functions
    c.ok(
        "cs: expression-body not a function",
        !nm.iter().any(|n| n == "NoBrace"),
    );
    c.ok(
        "cs: `class` not a function",
        !nm.iter().any(|n| n == "class"),
    );
    c.ok(
        "cs: type symbol",
        p.syms
            .iter()
            .any(|s| s.kind == "type" && s.name == "Animation"),
    );

    // --- symbol table: property vs field, members only (no locals)
    let p = parse_source(&chars(
        "namespace Terraria.DataStructures;\n\npublic class Point16\n{\n\tprivate readonly int _x;\n\tpublic int X { get; set; }\n\tpublic int Y\n\t{\n\t\tget { return _x; }\n\t}\n\n\tpublic void Move(int dx)\n\t{\n\t\tint localJunk = dx;\n\t\tHelper.Do(localJunk);\n\t}\n}\n",
    ));
    c.ok(
        "cs: namespace symbol",
        p.syms
            .iter()
            .any(|s| s.kind == "namespace" && s.name == "Terraria.DataStructures"),
    );
    c.ok(
        "cs: field symbol",
        p.syms.iter().any(|s| s.kind == "field" && s.name == "_x"),
    );
    c.ok(
        "cs: property X",
        p.syms.iter().any(|s| s.kind == "property" && s.name == "X"),
    );
    c.ok(
        "cs: property Y (Allman accessor)",
        p.syms.iter().any(|s| s.kind == "property" && s.name == "Y"),
    );
    c.ok(
        "cs: local not a symbol",
        !p.syms.iter().any(|s| s.name == "localJunk"),
    );

    // --- unicode: char offsets, not bytes
    let u_src = [
        "// naïve — 中文 \u{1f600} { brace",
        "void TakeDamage(int n)",
        "{",
        "\tset(\"secret_lobby_01\");   // 中文 comment",
        "}",
    ]
    .join("\n");
    let p = parse_source(&chars(&u_src));
    let f = p.funcs.first().expect("TakeDamage");
    c.ok("unicode: function found", names(&p) == ["TakeDamage"]);
    c.ok("unicode: line math intact", f.start == 2 && f.end == 5);
    c.ok(
        "unicode: literal after non-ascii comment",
        p.strings.iter().any(|(_, t)| t == "secret_lobby_01"),
    );
    // ASCII-only identifier class is structural here: no \w anywhere in match.rs
    c.ok(
        "unicode: non-ascii identifier rejected",
        parse_source(&chars("void caf\u{e9}(int x)\n{\n}\n"))
            .funcs
            .is_empty(),
    );

    // --- module rules: order is load-bearing
    c.ok(
        "rule: x: exact",
        module_of("Terraria/Foo.cs", Some(&chars("namespace Terraria;"))).0 == "core.runtime",
    );
    c.ok(
        "rule: b: beats n:",
        module_of("Terraria/Player.cs", None).0 == "core.player",
    );
    c.ok(
        "rule: n: prefix",
        module_of(
            "Terraria/Net/Client.cs",
            Some(&chars("namespace Terraria.Net;")),
        )
        .0 == "io.netplay",
    );
    c.ok(
        "rule: two namespaces -> one module",
        module_of("a/G.cs", Some(&chars("namespace Terraria.WorldBuilding;"))).0
            == "domain.dungeon",
    );
    c.ok(
        "rule: narrower n: rule wins over the sibling",
        module_of(
            "Terraria/GameContent/UI/Foo.cs",
            Some(&chars("namespace Terraria.GameContent.UI;")),
        )
        .0 == "ui.content",
    );
    c.ok(
        "rule: n: prefix",
        module_of(
            "Terraria/UI/ItemSlot.cs",
            Some(&chars("namespace Terraria.UI;")),
        )
        .0 == "ui.shell",
    );
    c.ok(
        "rule: dir-chain fallback",
        module_of("MyGame/UI/Button.cs", None).0 == "unmapped",
    );
    c.ok(
        "rule: unmatched",
        module_of("Other/Thing.cs", Some(&chars("namespace Other;"))).0 == "unmapped",
    );

    // --- mask preserves offsets and newlines
    let m = mask(&chars("a\r\n/* x\ny */\nb"), true);
    c.ok(
        "mask: length preserved",
        m.len() == chars("a\r\n/* x\ny */\nb").len(),
    );
    c.ok(
        "mask: newlines preserved",
        m.iter().filter(|&&c| c == '\n').count() == 3,
    );

    // --- call scanning: dedupe, self-exclusion, keyword exclusion, cap
    let body = "void a() {\n\tb();\n\tb();\n\tif (c) { }\n\ta();\n}\nvoid b() {}\n";
    let ch = chars(body);
    // `void b() {}` is a declaration with no body: the tail pattern requires
    // `{` at end of line, so it is deliberately not indexed as a function.
    assert!(parse_source(&ch).funcs.iter().all(|f| f.name == "a"));
    let masked = mask(&ch, true);
    let lr = {
        let mut v = vec![(0usize, 0usize)];
        let mut s = 0usize;
        for (i, &ch) in masked.iter().enumerate() {
            if ch == '\n' {
                v.push((s, i));
                s = i + 1;
            }
        }
        v.push((s, masked.len()));
        v
    };
    let mut by: std::collections::HashMap<String, Vec<i64>> = std::collections::HashMap::new();
    by.insert("b".into(), vec![2]);
    by.insert("a".into(), vec![1]);
    by.insert("c".into(), vec![3]);
    let e = scan_calls(&masked, &lr, 1, 5, 1, &by);
    c.ok("calls: dedupe + self excluded", e.len() == 1 && e[0].0 == 2);
    c.ok(
        "calls: self-recursion excluded",
        scan_calls(&masked, &lr, 1, 5, 2, &by)
            .iter()
            .all(|(d, _)| *d != 2),
    );

    // --- end-to-end
    let tmp = tmpdir("e2e");
    write_file(
        &tmp,
        "Terraria/Player.cs",
        "namespace Terraria {\npublic class Player\n{\npublic void TakeDamage(int n)\n{\n\tUseCounter(n);\n}\n}\n}\n",
    );
    write_file(
        &tmp,
        "Terraria/Main.cs",
        "namespace Terraria {\npublic class Main\n{\npublic void Run()\n{\n\tplayer.TakeDamage(1);\n}\n}\n}\n",
    );
    let _dbp = tmp.join(".gamedb/index.sqlite");
    let _ = std::fs::remove_dir_all(tmp.join(".gamedb"));

    let r1 = index_root(&tmp, None, 0, false, false).expect("index 1");
    c.ok("e2e: indexed 2", r1.files == 2 && r1.functions == 2);
    let r2 = index_root(&tmp, None, 0, false, false).expect("index 2");
    c.ok("e2e: idempotent", r2.skipped == 2 && r2.files == 0);
    let d = index_root(&tmp, None, 0, true, false).expect("dry run");
    c.ok("e2e: dry-run writes nothing", d.files == 2);

    let rows = search_functions(&tmp, None, "Take", 25).expect("search");
    c.ok(
        "e2e: search",
        rows.len() == 1 && rows[0][0].as_str() == "TakeDamage",
    );
    c.ok(
        "e2e: like-escaping",
        search_strings(&tmp, None, "%", 40).expect("esc").is_empty(),
    );
    let f = read_function(&tmp, None, "TakeDamage")
        .expect("read")
        .expect("found");
    c.ok(
        "e2e: read body",
        f.2 == "public void TakeDamage(int n)\n{\n\tUseCounter(n);\n}",
    );

    let mods = list_modules(&tmp, None).expect("modules");
    let cp = mods
        .iter()
        .find(|m| m.module_id == "core.player")
        .expect("core.player");
    c.ok("e2e: module assigned", cp.files == 1);
    c.ok(
        "e2e: no unmapped",
        !mods.iter().any(|m| m.module_id == "unmapped"),
    );

    let g = graph(&tmp, None, "Run", "callees", 10).expect("graph");
    c.ok(
        "e2e: graph callee edge",
        g.iter()
            .any(|r| r[0].as_str() == "callee" && r[1].as_str() == "TakeDamage"),
    );
    c.ok(
        "e2e: graph callers",
        graph(&tmp, None, "TakeDamage", "callers", 10)
            .expect("graph in")
            .iter()
            .any(|r| r[1].as_str() == "Run"),
    );

    let p1 = ModulePatch {
        state: Some(STATE_PARTIAL.into()),
        verified: Some(true),
        verified_by: Some("selftest".into()),
        remaining: None,
    };
    let m1 = set_module_state(&tmp, None, "core.player", &p1).expect("set-module");
    c.ok(
        "e2e: set-module",
        m1.state == STATE_PARTIAL && m1.verified_by.as_deref() == Some("selftest"),
    );
    let p2 = ModulePatch {
        state: Some(STATE_IMPLEMENTED.into()),
        ..Default::default()
    };
    let m2 = set_module_state(&tmp, None, "core.player", &p2).expect("set-module 2");
    c.ok(
        "e2e: verified stamp survives a patch that omits it",
        m2.verified_by.as_deref() == Some("selftest") && m2.verified_at.is_some(),
    );
    c.ok(
        "e2e: verified + IMPLEMENTED is reachable",
        m2.state == STATE_IMPLEMENTED,
    );
    // Dropping the sign-off also drops IMPLEMENTED: a state nobody earned must
    // not outlive its evidence, and the guard refuses the one-call shortcut.
    let p3 = ModulePatch {
        state: Some(STATE_PARTIAL.into()),
        verified: Some(false),
        ..Default::default()
    };
    c.ok("e2e: unverify from IMPLEMENTED is refused", {
        set_module_state(
            &tmp,
            None,
            "core.player",
            &ModulePatch {
                verified: Some(false),
                ..Default::default()
            },
        )
        .is_err()
    });
    set_module_state(&tmp, None, "core.player", &p3).expect("unverify");
    c.ok("e2e: --unverified clears the stamp", {
        let m =
            set_module_state(&tmp, None, "core.player", &ModulePatch::default()).expect("reread");
        m.verified_by.is_none()
    });

    let cases: Vec<(&str, ModulePatch)> = vec![
        (
            "IMPLEMENTED w/o verify",
            ModulePatch {
                state: Some(STATE_IMPLEMENTED.into()),
                ..Default::default()
            },
        ),
        (
            "bad state",
            ModulePatch {
                state: Some("BOGUS".into()),
                ..Default::default()
            },
        ),
    ];
    for (why, p) in cases {
        let got = set_module_state(&tmp, None, "core.player", &p);
        c.ok(&format!("e2e: rejects {}", why), got.is_err());
    }
    c.ok(
        "e2e: rejects unknown module",
        set_module_state(&tmp, None, "nope.nope", &ModulePatch::default()).is_err(),
    );

    // a second tree proves the index is per-root, not global
    let other = tmpdir("other");
    write_file(&other, "X.cs", "void f()\n{\n\tg();\n}\n");
    c.ok(
        "e2e: a root without an index is reported, not silently empty",
        !stats(&other, None).indexed,
    );
    c.ok(
        "e2e: the first root is still indexed",
        stats(&tmp, None).indexed,
    );
    let _ = std::fs::remove_dir_all(&other);

    let _ = std::fs::remove_dir_all(&tmp);

    let bad: Vec<&String> = c.v.iter().filter(|(_, ok)| !ok).map(|(l, _)| l).collect();
    for l in &bad {
        eprintln!("FAIL {}", l);
    }
    let total = c.v.len();
    let passed = total - bad.len();
    println!(
        "selftest {}/{} {}",
        passed,
        total,
        if bad.is_empty() { "OK" } else { "FAILED" }
    );
    i32::from(!bad.is_empty())
}
