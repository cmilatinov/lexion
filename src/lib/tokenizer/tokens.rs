mod regexes {
    use regex::Regex;
    use lazy_static::lazy_static;
    lazy_static! {
        pub static ref UNEXPECTED: Regex = Regex::new(r"^\S+").unwrap();
        pub static ref WHITESPACE: Regex = Regex::new(r"^\s+").unwrap();
        pub static ref SINGLE_LINE_COMMENT: Regex = Regex::new(r"^\/\/.*").unwrap();
        pub static ref MULTI_LINE_COMMENT: Regex = Regex::new(r"^\/\*[\s\S]*?\*\/").unwrap();
        pub static ref INTEGER: Regex = Regex::new(r"^(?:[1-9][0-9]*|0)").unwrap();
        pub static ref FLOAT: Regex = Regex::new(
            r"^[+\-]?(?:[1-9][0-9]*|0)?(?:\.[0-9]*[1-9]|\.0)(?:[eE][+\-]?(?:[1-9][0-9]*|0))?"
        ).unwrap();
        pub static ref BOOLEAN: Regex = Regex::new(r"^(?:true|false)").unwrap();
        pub static ref SINGLE_QUOTED_STRING: Regex = Regex::new(r"^'[^']*'").unwrap();
        pub static ref DOUBLE_QUOTED_STRING: Regex = Regex::new("^\"[^\"]*\"").unwrap();
    }
}

pub static EPSILON: &str = "ε";
pub static EOF: &str = "$";

pub use self::regexes::*;