#![recursion_limit = "1024"]

#[macro_use] extern crate error_chain;
extern crate itertools;
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
use itertools::Itertools;

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
    config.into_iter().fold(Ok(BTreeMap::new()), |mut result, (k, v)| {
        match load_env_variable(v) {
            Ok(Some(new_v)) => {
                if let Ok(ref mut m) = result.as_mut() {
                    m.insert(k, new_v);
                }
                result
            },
            Ok(None) =>
                result,
            Err(e) => {
                match result {
                    Ok(_) => Err(e),
                    Err(existing_err) => Err(combine_errors(existing_err, e))
                }
            }
        }
    }).map(|table| toml::Value::Table(table))
}

fn load_env_variable(value: toml::Value) -> Result<Option<toml::Value>, Error> {
    match value {
        toml::Value::String(ref s) if ENV_FLAG_REQ.is_match(s) => {
            let env_key = s
                .trim_left_matches("<<ENV:")
                .trim_right_matches(">>");

            match env::var(env_key) {
                Ok(env_var) =>
                    Ok(Some(toml::Value::String(env_var))),
                Err(env::VarError::NotPresent) =>
                    Err(ErrorKind::EnvVarMissing(env_key.to_owned()).into()),
                Err(e) =>
                    Err(e.into())
            }
        },
        toml::Value::String(ref s) if ENV_FLAG_OPT.is_match(&s) => {
            let env_key = s
                .trim_left_matches("<<ENV?:")
                .trim_right_matches(">>");
            match env::var(env_key) {
                Ok(env_var) =>
                    Ok(Some(toml::Value::String(env_var))),
                Err(env::VarError::NotPresent) =>
                    Ok(None),
                Err(e) =>
                    Err(e.into())
            }
        },
        toml::Value::Table(table) =>
            load_env_variables(table).map(|vs| Some(vs)),
        other_value =>
            Ok(Some(other_value))
    }
}

fn combine_errors(e1: Error, e2: Error) -> Error {
    match (e1, e2) {
        (Error(ErrorKind::Multiple(mut es1), _), Error(ErrorKind::Multiple(es2), _)) => {
            es1.extend(es2);
            ErrorKind::Multiple(es1).into()
        },
        (Error(ErrorKind::Multiple(mut es), _), other) |
        (other, Error(ErrorKind::Multiple(mut es), _)) => {
            es.push(other);
            ErrorKind::Multiple(es).into()
        },
        (e1, e2) => {
            ErrorKind::Multiple(vec![e1, e2]).into()
        }
    }
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

    errors {
        EnvVarMissing(key: String) {
            description("Required environment variable missing")
            display("Required environment variable '{}' not set", key)
        }
        Multiple(errs: Vec<Error>) {
            description("Multiple errors")
            display("Errors: {}", errs.iter().join(", "))
        }
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
            foo = "<<ENV:FOO1>>"
            bar = 1234
            baz = "baz value"
            [more]
            thing1 = "thing1 value"
            thing2 = "thing2 value"
        "#;

        env::set_var("FOO1", "env foo value");

        let config: Config = load_config_from_str(config_str).unwrap();
        assert_eq!(&config.foo, "env foo value");
        assert_eq!(config.bar, 1234);
        assert_eq!(&config.baz, &Some("baz value".to_string()));
        assert_eq!(&config.more.thing1, "thing1 value");
        assert_eq!(&config.more.thing2, "thing2 value");
    }

    #[test]
    fn it_fails_when_required_env_var_missing() {
        let config_str = r#"
            foo = "<<ENV:FOO2>>"
            bar = 1234
            baz = "<<ENV:BAZ2>>"
            [more]
            thing1 = "thing1 value"
            thing2 = "thing2 value"
        "#;

        assert!(load_config_from_str::<Config>(config_str).is_err())
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
