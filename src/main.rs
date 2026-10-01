mod cli;
mod db;
mod modules;
mod parse;
mod rx;
mod selftest;
mod store;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let rc = match cli::parse(argv) {
        Ok(a) => {
            if a.help {
                print!("{}", cli::USAGE);
                0
            } else if a.version {
                println!("{}", env!("CARGO_PKG_VERSION"));
                0
            } else {
                match cli::run(a) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("gamedb: {}", e);
                        1
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("gamedb: {}\n\n{}", e, cli::USAGE);
            2
        }
    };
    std::process::exit(rc);
}
