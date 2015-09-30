#[macro_use]
extern crate log;
extern crate env_logger;
extern crate rustc_serialize;
extern crate docopt;
extern crate glob;
extern crate toml;

use docopt::Docopt;
use std::path::Path;
use std::fs::File;
use std::fs::metadata;
use std::io::prelude::*;
use glob::Pattern;
use rustc_serialize::Decodable;

const USAGE: &'static str = "
enforcer for code rules

Usage:
  enforcer <glob> [-c|--clean]
  enforcer (-h | --help)
  enforcer (-v | --version)

Options:
  -h --help     Show this screen.
  -v --version  Show version.
  --count       only count found entries
  -c --clean    clean up trailing whitespaces
";
const VERSION: &'static str = env!("CARGO_PKG_VERSION");

#[derive(Debug, RustcDecodable)]
struct Args {
    flag_count: bool,
    flag_clean: bool,
    arg_glob: String,
    flag_version: bool,
}
const HAS_TABS: u8               = 1 << 0;
const TRAILING_SPACES: u8        = 1 << 1;
const HAS_ILLEGAL_CHARACTERS: u8 = 1 << 2;


fn check_content<'a>(input: &'a str) -> std::io::Result<u8> {
    let mut result = 0;
    for line in input.lines_any() {
        if line.ends_with(' ') {
            result |= TRAILING_SPACES;
        }
        if line.contains("\t") {
            result |= HAS_TABS;
        }
        if line.as_bytes().iter().any(|x| *x > 127) {
            result |= HAS_ILLEGAL_CHARACTERS;
        }
    }
    Ok(result)
}

fn clean_string(input: &str) -> String {
    let v: Vec<&str> = input
        .lines_any()
        .map(|line| line.trim_right())
        .collect();

    if input.ends_with("\n") {
        v.join("\n") + "\n"
    }
    else {
        v.join("\n")
    }
}

fn is_dir(path: &Path) -> bool {
    if let Ok(result) = metadata(&path) {
        result.is_dir()
    } else {
        false
    }
}

fn check_path(path: &Path, clean: bool) -> std::io::Result<()> {
    use std::io::ErrorKind;

    let mut f = try!(File::open(path));
    let mut buffer = String::new();
    let mut check = 0;
    if let Err(e) = f.read_to_string(&mut buffer) {
        match e.kind() {
            ErrorKind::InvalidData => check = check | HAS_ILLEGAL_CHARACTERS,
            _ => return Err(e),
        }
    }
    // only check content if we could read the file
    if check == 0 { check = try!(check_content(&buffer)); }
    if (check & HAS_ILLEGAL_CHARACTERS) > 0 {
        println!("HAS_ILLEGAL_CHARACTERS:[{}]", path.display());
    }
    if (check & HAS_TABS) > 0 {
        println!("HAS_TABS:[{}]", path.display());
    }
    if (check & TRAILING_SPACES) > 0 {
        println!("TRAILING_SPACES:[{}]", path.display());
        if clean {
            println!("cleaning trailing whitespaces");
            let res_string = clean_string(&buffer);
            let mut file = try!(File::create(path));
            try!(file.write_all(res_string.as_bytes()));
        }
    }
    Ok(())
}

#[derive(Debug, RustcDecodable, PartialEq)]
struct EnforcerCfg {
    ignore: Vec<String>,
}
fn default_cfg() -> EnforcerCfg {
    EnforcerCfg { ignore: [".git", ".bake"].iter().map(|x| x.to_string()).collect() }
}

fn is_unwanted(comp: std::path::Component, enforcer_cfg: &EnforcerCfg) -> bool {
    let path_elem = comp.as_os_str()
                        .to_str()
                        .expect(&format!("illegal path component {:?}", comp)[..]);
    enforcer_cfg.ignore.iter()
        .any(|x| Pattern::new(x)
            .ok()
            .expect(&format!("{} seems not to be a valid pattern", x)[..])
            .matches(path_elem))
}

fn load_config<'a>(input: &'a str) -> std::io::Result<EnforcerCfg> {
    use std::io::{Error, ErrorKind};
    let mut parser = toml::Parser::new(input);
    fn default_err() -> Error {
        Error::new(ErrorKind::InvalidData, "could not parse the config")
    }

    parser.parse().map_or(Err(default_err()), |toml| {
        let mut decoder = toml::Decoder::new(toml::Value::Table(toml));
        EnforcerCfg::decode(&mut decoder)
            .ok()
            .map_or(Err(default_err()), |config|
                Ok(config))
    })
}

#[allow(dead_code)]
fn main() {
    use glob::glob;
    env_logger::init().unwrap();

    fn get_cfg() -> EnforcerCfg {
        fn read_enforcer_config() -> std::io::Result<EnforcerCfg> {
            let mut cfg_file = try!(File::open(".enforcer"));
            let mut buffer = String::new();
            try!(cfg_file.read_to_string(&mut buffer));
            load_config(&buffer[..])
        }
        let enforcer_cfg = read_enforcer_config()
            .unwrap_or(default_cfg());
        println!("loaded ignores: {:?}", enforcer_cfg.ignore);
        enforcer_cfg
    }

    let args: Args = Docopt::new(USAGE)
                            .and_then(|d| d.decode())
                            .unwrap_or_else(|e| e.exit());

    if args.flag_version {
        println!("  Version: {}", VERSION);
    }
    let pat = args.arg_glob.to_string();

    let enforcer_cfg = get_cfg();
    for path in glob(&*pat)
        .ok()
        .expect(&format!("glob has problems with {}", pat)[..])
        .filter_map(Result::ok)
        .filter(|x| !x.components()
                        .any(|y| is_unwanted(y, &enforcer_cfg))) {
            if !is_dir(path.as_path()) {
                check_path(path.as_path(), args.flag_clean)
                    .ok()
                    .expect(&format!("check_path for {:?} should work", path));
            }
    }
}

#[cfg(test)]
mod tests {
    use super::clean_string;
    use super::load_config;
    use super::is_unwanted;
    use glob::Pattern;
    use super::EnforcerCfg;
    use std::ffi::OsStr;
    use std::path::Component::Normal;

    #[test]
    fn test_clean_does_not_remove_trailing_newline() {
        let content = "1\n2\n3\n4\n5\n";
        let cleaned = clean_string(content);
        assert!(cleaned.eq(content));
    }
    #[test]
    fn test_clean_trailing_whitespace() {
        let content = "1 \n2";
        let cleaned = clean_string(content);
        println!("{:?}", cleaned);
        assert!(cleaned.eq("1\n2"));
    }
    #[test]
    fn test_load_simple_config() {
        let c = include_str!("../samples/.enforcer");
        let cfg = load_config(c).unwrap();
        assert_eq!(cfg.ignore.len(), 2);
        let expected = EnforcerCfg { ignore: [".git", ".repo"].iter().map(|x| x.to_string()).collect() };
        assert_eq!(expected.ignore, cfg.ignore);
        assert_eq!(expected, cfg);
    }
    #[test]
    fn test_load_broken_config() {
        let c = include_str!("../samples/.enforcer_broken");
        let cfg = load_config(c).unwrap();
        let expected = EnforcerCfg { ignore: [".git", ".repo"].iter().map(|x| x.to_string()).collect() };
        assert!(expected.ignore != cfg.ignore);
    }
    #[test]
    fn test_glob() {
        assert!(Pattern::new("build_*").unwrap().matches("build_Debug"));
    }
    #[test]
    fn test_is_unwanted() {
        let cfg = EnforcerCfg { ignore: vec!["build_*".to_string(), ".git".to_string()]};
        assert!(is_unwanted(Normal(OsStr::new("build_Debug")), &cfg));
        assert!(is_unwanted(Normal(OsStr::new(".git")), &cfg));
        assert!(!is_unwanted(Normal(OsStr::new("bla")), &cfg));
    }
}

