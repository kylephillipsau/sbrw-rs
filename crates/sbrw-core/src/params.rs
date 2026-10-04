//! `ParameterBO`: server settings from the `parameter` table.
//!
//! The values are upstream's `Data.sql` rows, overridden by `[parameters]` in
//! the config file (and by scenarios in replays). Lookups keep upstream's
//! semantics, including which ones throw when a parameter is missing.

use std::collections::HashMap;

use anyhow::anyhow;
use rusqlite::Connection;

use crate::Fault;
use crate::dao::user::UserEntity;

// port: bo/ParameterBO#<init>()
// port: dao/ParameterDAO#<init>()
#[derive(Debug, Clone, Default)]
pub struct Parameters {
    map: HashMap<String, String>,
}

impl Parameters {
    // port: bo/ParameterBO#init()
    // port: bo/ParameterBO#loadParameters()
    // port: dao/ParameterDAO#findAll()
    pub fn load(db: &Connection) -> rusqlite::Result<Self> {
        let mut stmt = db.prepare("SELECT name, value FROM parameter")?;
        let map = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?
            .filter_map(|row| match row {
                Ok((name, Some(value))) => Some(Ok((name, value))),
                Ok((_, None)) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<rusqlite::Result<_>>()?;
        Ok(Self { map })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.map.get(name).map(String::as_str)
    }

    // port: bo/ParameterBO#getIntParam(String)
    /// Throws when the parameter is missing or not an integer.
    pub fn get_int_param(&self, name: &str) -> Result<i32, Fault> {
        let value = self.get(name).ok_or_else(|| anyhow!("Cannot find integer parameter: {name}"))?;
        java_int(value)
    }

    // port: bo/ParameterBO#getIntParam(String,Integer)
    /// `default` when missing or empty; throws when present but not an integer.
    pub fn get_int_param_or(&self, name: &str, default: i32) -> Result<i32, Fault> {
        match self.get(name) {
            None | Some("") => Ok(default),
            Some(value) => java_int(value),
        }
    }

    // port: bo/ParameterBO#getBoolParam(String)
    /// `Boolean.valueOf`: true only for "true", ignoring case.
    pub fn get_bool_param(&self, name: &str) -> bool {
        self.get(name).is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }

    // port: bo/ParameterBO#getStrParam(String)
    pub fn get_str_param(&self, name: &str) -> Option<String> {
        self.get(name).map(str::to_owned)
    }

    // port: bo/ParameterBO#getStrParam(String,String)
    pub fn get_str_param_or(&self, name: &str, default: Option<&str>) -> Option<String> {
        match self.get(name) {
            None | Some("") => default.map(str::to_owned),
            Some(value) => Some(value.to_owned()),
        }
    }

    // port: bo/ParameterBO#getStrListParam(String)
    /// `value.split(";")`, which drops trailing empty entries.
    pub fn get_str_list_param(&self, name: &str) -> Vec<String> {
        match self.get(name) {
            None | Some("") => Vec::new(),
            Some(value) => {
                let mut parts: Vec<String> = value.split(';').map(str::to_owned).collect();
                while parts.len() > 1 && parts.last().is_some_and(String::is_empty) {
                    parts.pop();
                }
                if parts.len() == 1 && parts[0].is_empty() {
                    parts.clear();
                }
                parts
            }
        }
    }

    // port: bo/ParameterBO#getFloatParam(String)
    pub fn get_float_param(&self, name: &str) -> Result<f32, Fault> {
        let value = self.get(name).ok_or_else(|| anyhow!("Cannot find float parameter: {name}"))?;
        java_float(value)
    }

    // port: bo/ParameterBO#getFloatParam(String,Float)
    pub fn get_float_param_or(&self, name: &str, default: f32) -> Result<f32, Fault> {
        match self.get(name) {
            None | Some("") => Ok(default),
            Some(value) => java_float(value),
        }
    }

    // port: bo/ParameterBO#getCarLimit(UserEntity)
    pub fn get_car_limit(&self, user: &UserEntity) -> Result<i32, Fault> {
        let name = if user.premium { "MAX_CAR_SLOTS_PREMIUM" } else { "MAX_CAR_SLOTS_FREE" };
        self.get_int_param_or(name, 200)
    }

    // port: bo/ParameterBO#getMaxCash(UserEntity)
    pub fn get_max_cash(&self, user: &UserEntity) -> Result<i32, Fault> {
        let name = if user.premium { "MAX_PLAYER_CASH_PREMIUM" } else { "MAX_PLAYER_CASH_FREE" };
        self.get_int_param_or(name, 9_999_999)
    }

    // port: bo/ParameterBO#getMaxLevel(UserEntity)
    pub fn get_max_level(&self, user: &UserEntity) -> Result<i32, Fault> {
        let name = if user.premium { "MAX_PLAYER_LEVEL_PREMIUM" } else { "MAX_PLAYER_LEVEL_FREE" };
        self.get_int_param_or(name, 60)
    }
}

/// `Integer.valueOf`: optional sign, decimal digits, in range; nothing else.
fn java_int(value: &str) -> Result<i32, Fault> {
    value.parse().map_err(|_| Fault::Internal(anyhow!("For input string: \"{value}\"")))
}

/// `Float.valueOf`.
fn java_float(value: &str) -> Result<f32, Fault> {
    sbrw_java::lang::parse_float(value).ok_or_else(|| Fault::Internal(anyhow!("For input string: \"{value}\"")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Parameters {
        Parameters { map: pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect() }
    }

    #[test]
    fn upstream_semantics() {
        let p = params(&[("N", "42"), ("E", ""), ("B", "TRUE"), ("L", "a;b;;"), ("F", " 1.5f "), ("X", "4x")]);
        assert_eq!(p.get_int_param("N").unwrap(), 42);
        assert!(p.get_int_param("missing").is_err());
        assert!(p.get_int_param("X").is_err());
        assert_eq!(p.get_int_param_or("E", 7).unwrap(), 7);
        assert_eq!(p.get_int_param_or("missing", 7).unwrap(), 7);
        assert!(p.get_int_param_or("X", 7).is_err());
        assert!(p.get_bool_param("B"));
        assert!(!p.get_bool_param("missing"));
        assert_eq!(p.get_str_param("E").as_deref(), Some(""));
        assert_eq!(p.get_str_param_or("E", Some("d")).as_deref(), Some("d"));
        assert_eq!(p.get_str_list_param("L"), ["a", "b"]);
        assert!(p.get_str_list_param("missing").is_empty());
        assert_eq!(p.get_float_param("F").unwrap(), 1.5);
    }

    #[test]
    fn loads_upstreams_rows() {
        let db = crate::store::open(None, &Default::default()).unwrap();
        let p = Parameters::load(&db).unwrap();
        assert_eq!(p.get_int_param_or("SESSION_LENGTH_MINUTES", 130).unwrap(), 1440);
        assert!(p.get_str_param("TICKET_TOKEN").is_none());
    }
}
