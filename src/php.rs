use crate::Error;
use php_literal_parser::{Key, Value};
use std::path::Path;
use std::process::Command;
use tracing::{debug, warn};

fn from_json(json: serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else {
                // > i64::MAX
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(a) => Value::Array(
            a.into_iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), from_json(v)))
                .collect(),
        ),
        serde_json::Value::Object(o) => Value::Array(
            o.into_iter()
                .map(|(i, v)| (Key::String(i), from_json(v)))
                .collect(),
        ),
    }
}

#[tracing::instrument(skip_all, fields(path = %path.as_ref().display()))]
pub fn try_exec_config<P: AsRef<Path>>(path: P) -> Result<Value, Error> {
    debug!("Attempting executing the config file php as fallback");
    let path = path.as_ref();
    let cmd = Command::new("php")
        .arg("-r")
        .arg(format!(
            r#"
            include "{}";
            echo json_encode($CONFIG);
            "#,
            path.display()
        ))
        .output()
        .map_err(|e| {
            warn!(
                config_file = %path.display(),
                error = %e,
                "error while executing config file with php"
            );
            Error::Exec
        })?;
    let stdout = cmd.stdout;
    let json: serde_json::Value = serde_json::from_slice(&stdout).map_err(|_e| {
        warn!(
            config_file = %path.display(),
            json = ?std::str::from_utf8(&stdout),
            "php returned invalid json"
        );
        Error::Exec
    })?;
    Ok(from_json(json))
}

#[cfg(test)]
use crate::nc::parse_db_options;
#[cfg(test)]
use crate::{assert_debug_equal, Database, DbConnect, SslOptions};

#[test]
fn test_parse_redis_socket() {
    let database =
        parse_db_options(&try_exec_config("tests/configs/non_literal.php").unwrap()).unwrap();
    assert_debug_equal(
        &Database::MySql {
            database: "foo_db".to_string(),
            username: "nextcloud".to_string(),
            password: "secret".to_string(),
            connect: DbConnect::Tcp {
                host: "127.0.0.1".to_string(),
                port: 3306,
            },
            ssl_options: SslOptions::Disabled,
        },
        &database,
    );
}
