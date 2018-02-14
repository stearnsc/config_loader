#![recursion_limit = "1024"]

#[macro_use] extern crate error_chain;
#[macro_use] extern crate lazy_static;
extern crate regex;
extern crate serde;
extern crate toml;

#[cfg(test)]
#[macro_use]
extern crate serde_derive;


use std::fs::File;
use std::env;
use std::io::Read;
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};
use regex::Regex;
use std::collections::BTreeMap;

lazy_static! {
    static ref ENV_FLAG_REQ: Regex = Regex::new("^<<ENV:([a-zA-Z0-9_]*)>>$").unwrap();
    static ref ENV_FLAG_OPT: Regex = Regex::new("^<<ENV\\?:([a-zA-Z0-9_]*)>>$").unwrap();
}


pub fn load_config<C: DeserializeOwned, P: AsRef<Path>>(config_path: Option<P>) -> Result<C, Error> {
    let mut config_file = open_config_file(config_path)?;
    let mut s = String::new();
    config_file.read_to_string(&mut s)?;

    load_config_from_str(&s)
}

pub fn load_config_from_str<C: DeserializeOwned>(config_str: &str) -> Result<C, Error> {
    let loaded_config = load_env_variables(toml::from_str(config_str)?)?;

    // is there a better way to do this than shortcutting through string?
    let loaded_config_str = toml::to_string(&loaded_config)?;
    let config = toml::from_str(&loaded_config_str)?;

    Ok(config)
}

fn open_config_file<T: AsRef<Path>>(path: Option<T>) -> Result<File, Error> {
    match path {
        Some(path) => File::open(path),
        None => {
            let default_path = get_default_config_path()
                .ok_or(String::from("Default config file not found"))?;
            File::open(default_path)
        }
    }.map_err(|e| e.into())
}


fn get_default_config_path() -> Option<PathBuf> {
    let mut path = env::current_dir()
        .expect("Error finding executable directory");
    path.push("Config.toml");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn load_env_variables(config: toml::value::Table) -> Result<toml::Value, Error> {
    config.into_iter().fold(Ok(BTreeMap::new()), |result, (k, v)| {
        result.and_then(|mut acc| {
            match v {
                toml::Value::String(ref s) if ENV_FLAG_REQ.is_match(s) => {
                    let env_key = s
                        .trim_left_matches("<<ENV:")
                        .trim_right_matches(">>");
                    let env_var = env::var(env_key)?;
                    acc.insert(k, toml::Value::String(env_var));
                },
                toml::Value::String(ref s) if ENV_FLAG_OPT.is_match(&s) => {
                    let env_key = s
                        .trim_left_matches("<<ENV?:")
                        .trim_right_matches(">>");
                    if let Ok(env_var) = env::var(env_key) {
                        acc.insert(k, toml::Value::String(env_var));
                    }
                },
                toml::Value::Table(table) => {
                    acc.insert(k, load_env_variables(table)?);
                },
                other_value => {
                    acc.insert(k, other_value);
                }
            }

            Ok(acc)
        })
    }).map(|table| toml::Value::Table(table))
}

error_chain! {
     types {
        Error, ErrorKind, ResultExt;
     }

    foreign_links {
        Io(::std::io::Error);
        Env(env::VarError);
        Deserialization(toml::de::Error);
        Serialization(toml::ser::Error);
    }
}


#[cfg(test)]
mod tests {
    use super::load_config_from_str;
    use std::env;

    #[derive(Debug, Deserialize)]
    struct SubConfig {
        thing1: String,
        thing2: String
    }

    #[derive(Debug, Deserialize)]
    struct Config {
        foo: String,
        bar: i32,
        baz: Option<String>,
        more: SubConfig
    }


    #[test]
    fn it_works_when_all_defined() {
        let config_str = r#"
            foo = "foo value"
            bar = 1234
            baz = "baz value"
            [more]
            thing1 = "thing1 value"
            thing2 = "thing2 value"
        "#;

        let config: Config = load_config_from_str(config_str).unwrap();
        assert_eq!(&config.foo, "foo value");
        assert_eq!(config.bar, 1234);
        assert_eq!(&config.baz, &Some("baz value".to_string()));
        assert_eq!(&config.more.thing1, "thing1 value");
        assert_eq!(&config.more.thing2, "thing2 value");
    }

    #[test]
    fn it_works_when_opt_empty() {
        let config_str = r#"
            foo = "foo value"
            bar = 1234
            [more]
            thing1 = "thing1 value"
            thing2 = "thing2 value"
        "#;

        let config: Config = load_config_from_str(config_str).unwrap();
        assert_eq!(&config.foo, "foo value");
        assert_eq!(config.bar, 1234);
        assert_eq!(&config.baz, &None);
        assert_eq!(&config.more.thing1, "thing1 value");
        assert_eq!(&config.more.thing2, "thing2 value");
    }

    #[test]
    fn it_works_when_env_var_required() {
        let config_str = r#"
            foo = "<<ENV:FOO>>"
            bar = 1234
            baz = "baz value"
            [more]
            thing1 = "thing1 value"
            thing2 = "thing2 value"
        "#;

        env::set_var("FOO", "env foo value");

        let config: Config = load_config_from_str(config_str).unwrap();
        assert_eq!(&config.foo, "env foo value");
        assert_eq!(config.bar, 1234);
        assert_eq!(&config.baz, &Some("baz value".to_string()));
        assert_eq!(&config.more.thing1, "thing1 value");
        assert_eq!(&config.more.thing2, "thing2 value");
    }

    //Tests combined here to avoid race condition between accessing env
    #[test]
    fn it_works_when_env_var_optional() {
        let config_str = r#"
            foo = "foo value"
            bar = 1234
            baz = "<<ENV?:BAZ>>"
            [more]
            thing1 = "thing1 value"
            thing2 = "thing2 value"
        "#;

        env::set_var("BAZ", "env baz value");

        let config1: Config = load_config_from_str(config_str).unwrap();
        assert_eq!(&config1.foo, "foo value");
        assert_eq!(config1.bar, 1234);
        assert_eq!(&config1.baz, &Some("env baz value".to_string()));
        assert_eq!(&config1.more.thing1, "thing1 value");
        assert_eq!(&config1.more.thing2, "thing2 value");

        env::remove_var("BAZ");

        let config2: Config = load_config_from_str(config_str).unwrap();
        assert_eq!(&config2.foo, "foo value");
        assert_eq!(config2.bar, 1234);
        assert_eq!(&config2.baz, &None);
        assert_eq!(&config2.more.thing1, "thing1 value");
        assert_eq!(&config2.more.thing2, "thing2 value");
    }
}
