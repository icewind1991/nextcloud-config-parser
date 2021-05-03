use crate::{Config, Error, NotAConfigError, RedisConfig, Result};
use php_literal_parser::Value;
use redis::{ConnectionAddr, ConnectionInfo};
use sqlx::any::AnyConnectOptions;
use sqlx::mysql::{MySqlConnectOptions, MySqlSslMode};
use sqlx::postgres::PgConnectOptions;
use sqlx::sqlite::SqliteConnectOptions;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

static CONFIG_CONSTANTS: &[(&str, &str)] = &[
    (r"\RedisCluster::FAILOVER_NONE", "0"),
    (r"\RedisCluster::FAILOVER_ERROR", "1"),
    (r"\RedisCluster::DISTRIBUTE", "2"),
    (r"\RedisCluster::FAILOVER_DISTRIBUTE_SLAVES", "3"),
];

fn glob_config_files(path: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut configs = vec![path.as_ref().into()];
    if let Some(parent) = path.as_ref().parent() {
        if let Ok(dir) = parent.read_dir() {
            for file in dir.filter_map(Result::ok) {
                let path = file.path();
                match path.to_str() {
                    Some(path_str) if path_str.ends_with(".config.php") => {
                        configs.push(path);
                    }
                    _ => {}
                }
            }
        }
    }
    configs
}

fn parse_php(path: impl AsRef<Path>) -> Result<Value> {
    let mut content = std::fs::read_to_string(&path)
        .map_err(|err| Error::ReadFailed(err, path.as_ref().into()))?;

    for (search, replace) in CONFIG_CONSTANTS {
        if content.contains(search) {
            content = content.replace(search, replace);
        }
    }

    let php = match content.find("$CONFIG") {
        Some(pos) => content[pos + "$CONFIG".len()..]
            .trim()
            .trim_start_matches('='),
        None => {
            return Err(Error::NotAConfig(NotAConfigError::NoConfig(
                path.as_ref().into(),
            )));
        }
    };
    Ok(php_literal_parser::from_str(php)?)
}

fn merge_configs(input: Vec<(PathBuf, Value)>) -> Result<Value> {
    let mut merged = HashMap::with_capacity(16);

    for (path, config) in input {
        match config.into_hashmap() {
            Some(map) => {
                for (key, value) in map {
                    merged.insert(key, value);
                }
            }
            None => {
                return Err(Error::NotAConfig(NotAConfigError::NotAnArray(path)));
            }
        }
    }

    Ok(Value::Array(merged))
}

pub fn parse(path: impl AsRef<Path>) -> Result<Config> {
    let files = glob_config_files(path);
    let parsed_files = files
        .into_iter()
        .map(|path| {
            let parsed = parse_php(&path)?;
            Result::<_, Error>::Ok((path, parsed))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let parsed = merge_configs(parsed_files)?;

    let database = parse_db_options(&parsed)?;
    let database_prefix = parsed["dbtableprefix"]
        .as_str()
        .unwrap_or("oc_")
        .to_string();
    let nextcloud_url = parsed["overwrite.cli.url"]
        .clone()
        .into_string()
        .ok_or(Error::NoUrl)?;
    let redis = parse_redis_options(&parsed);

    Ok(Config {
        database,
        database_prefix,
        nextcloud_url,
        redis,
    })
}

fn parse_db_options(parsed: &Value) -> Result<AnyConnectOptions> {
    match parsed["dbtype"].as_str() {
        Some("mysql") => {
            let mut options = MySqlConnectOptions::new();
            if let Some(username) = parsed["dbuser"].as_str() {
                options = options.username(username);
            }
            if let Some(password) = parsed["dbpassword"].as_str() {
                options = options.password(password);
            }
            let socket_addr1 = PathBuf::from("/var/run/mysqld/mysqld.sock");
            let socket_addr2 = PathBuf::from("/tmp/mysql.sock");
            let socket_addr3 = PathBuf::from("/run/mysql/mysql.sock");
            match split_host(parsed["dbhost"].as_str().unwrap_or_default()) {
                ("localhost", None, None) if socket_addr1.exists() => {
                    options = options.socket(socket_addr1);
                }
                ("localhost", None, None) if socket_addr2.exists() => {
                    options = options.socket(socket_addr2);
                }
                ("localhost", None, None) if socket_addr3.exists() => {
                    options = options.socket(socket_addr3);
                }
                (addr, None, None) => {
                    options = options.host(addr);
                    if IpAddr::from_str(addr).is_ok() {
                        options = options.ssl_mode(MySqlSslMode::Disabled);
                    }
                }
                (addr, Some(port), None) => {
                    options = options.host(addr).port(port);
                    if IpAddr::from_str(addr).is_ok() {
                        options = options.ssl_mode(MySqlSslMode::Disabled);
                    }
                }
                (_, None, Some(socket)) => {
                    options = options.socket(socket);
                }
                (_, Some(_), Some(_)) => {
                    unreachable!()
                }
            }
            if let Some(port) = parsed["dbport"].clone().into_int() {
                options = options.port(port as u16);
            }
            if let Some(name) = parsed["dbname"].as_str() {
                options = options.database(name);
            }

            Ok(options.into())
        }
        Some("pgsql") => {
            let mut options = PgConnectOptions::new();
            if let Some(username) = parsed["dbuser"].as_str() {
                options = options.username(username);
            }
            if let Some(password) = parsed["dbpassword"].as_str() {
                options = options.password(password);
            }
            match split_host(parsed["dbhost"].as_str().unwrap_or_default()) {
                (addr, None, None) => {
                    options = options.host(addr);
                }
                (addr, Some(port), None) => {
                    options = options.host(addr).port(port);
                }
                (_, None, Some(socket)) => {
                    let mut socket_path = Path::new(socket);

                    // sqlx wants the folder the socket is in, not the socket itself
                    if socket_path
                        .file_name()
                        .map(|name| name.to_str().unwrap().starts_with(".s"))
                        .unwrap_or(false)
                    {
                        socket_path = socket_path.parent().unwrap();
                    }
                    options = options.socket(socket_path);
                }
                (_, Some(_), Some(_)) => {
                    unreachable!()
                }
            }
            if let Some(port) = parsed["dbport"].clone().into_int() {
                options = options.port(port as u16);
            }
            if let Some(name) = parsed["dbname"].as_str() {
                options = options.database(name);
            }
            Ok(options.into())
        }
        Some("sqlite3") => {
            let mut options = SqliteConnectOptions::new();
            if let Some(data_dir) = parsed["datadirectory"].as_str() {
                let db_name = parsed["dbname"]
                    .clone()
                    .into_string()
                    .unwrap_or_else(|| String::from("owncloud"));
                options = options.filename(format!("{}/{}.db", data_dir, db_name));
            }
            Ok(options.into())
        }
        Some(ty) => Err(Error::UnsupportedDb(ty.into())),
        None => Err(Error::NoDb),
    }
}

fn split_host(host: &str) -> (&str, Option<u16>, Option<&str>) {
    let mut parts = host.split(':');
    let host = parts.next().unwrap();
    match parts
        .next()
        .map(|port_or_socket| u16::from_str(port_or_socket).map_err(|_| port_or_socket))
    {
        Some(Ok(port)) => (host, Some(port), None),
        Some(Err(socket)) => (host, None, Some(socket)),
        None => (host, None, None),
    }
}

enum RedisAddress {
    Single(ConnectionAddr),
    Cluster(Vec<ConnectionAddr>),
}

fn parse_redis_options(parsed: &Value) -> RedisConfig {
    let (redis_options, address) = if parsed["redis.cluster"].is_array() {
        let redis_options = &parsed["redis.cluster"];
        let seeds = redis_options["seeds"].values();
        let addresses = seeds
            .filter_map(|seed| seed.as_str())
            .map(split_host)
            .filter_map(|(host, port, _)| Some(ConnectionAddr::Tcp(host.into(), port?)))
            .collect::<Vec<_>>();
        (redis_options, RedisAddress::Cluster(addresses))
    } else {
        let redis_options = &parsed["redis"];
        let mut host = redis_options["host"].as_str().unwrap_or("127.0.0.1");
        let address = if host.starts_with('/') {
            RedisAddress::Single(ConnectionAddr::Unix(host.into()))
        } else {
            if host == "localhost" {
                host = "127.0.0.1";
            }
            let (host, port, _) = if let Some(port) = redis_options["port"].as_int() {
                (host, Some(port as u16), None)
            } else {
                split_host(host)
            };
            RedisAddress::Single(ConnectionAddr::Tcp(host.into(), port.unwrap_or(6379)))
        };
        (redis_options, address)
    };

    let db = redis_options["dbindex"].clone().into_int().unwrap_or(0);
    let passwd = redis_options["password"]
        .as_str()
        .filter(|pass| !pass.is_empty())
        .map(String::from);

    match address {
        RedisAddress::Single(addr) => RedisConfig::Single(ConnectionInfo {
            addr: Box::new(addr),
            db,
            username: None,
            passwd,
        }),
        RedisAddress::Cluster(addresses) => RedisConfig::Cluster(
            addresses
                .into_iter()
                .map(|addr| ConnectionInfo {
                    addr: Box::new(addr),
                    db,
                    username: None,
                    passwd: passwd.clone(),
                })
                .collect(),
        ),
    }
}

#[test]
fn test_redis_empty_password_none() {
    let config =
        php_literal_parser::from_str(r#"["redis" => ["host" => "redis", "password" => "pass"]]"#)
            .unwrap();
    let redis = parse_redis_options(&config);
    assert_eq!(redis.passwd(), Some("pass"));

    let config =
        php_literal_parser::from_str(r#"["redis" => ["host" => "redis", "password" => ""]]"#)
            .unwrap();
    let redis = parse_redis_options(&config);
    assert_eq!(redis.passwd(), None);
}

#[cfg(test)]
#[track_caller]
fn assert_debug_equal<T: Debug, U: Debug>(a: T, b: U) {
    assert_eq!(format!("{:?}", a), format!("{:?}", b),);
}

#[cfg(test)]
use std::fmt::Debug;
use std::net::IpAddr;

#[cfg(test)]
fn config_from_file(path: &str) -> Config {
    parse(path).unwrap()
}

#[test]
fn test_parse_config_basic() {
    let config = config_from_file("tests/configs/basic.php");
    assert_eq!("https://cloud.example.com", config.nextcloud_url);
    assert_eq!("oc_", config.database_prefix);
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@127.0.0.1/nextcloud?ssl-mode=disabled",
        )
        .unwrap(),
        config.database,
    );
    assert_debug_equal(
        RedisConfig::Single(ConnectionInfo::from_str("redis://127.0.0.1").unwrap()),
        config.redis,
    );
}

#[test]
fn test_parse_implicit_prefix() {
    let config = config_from_file("tests/configs/implicit_prefix.php");
    assert_eq!("oc_", config.database_prefix);
}

#[test]
fn test_parse_empty_redis_password() {
    let config = config_from_file("tests/configs/empty_redis_password.php");
    assert_debug_equal(
        RedisConfig::Single(ConnectionInfo::from_str("redis://127.0.0.1").unwrap()),
        config.redis,
    );
}

#[test]
fn test_parse_full_redis() {
    let config = config_from_file("tests/configs/full_redis.php");
    assert_debug_equal(
        RedisConfig::Single(ConnectionInfo::from_str("redis://:moresecret@redis:1234/1").unwrap()),
        config.redis,
    );
}

#[test]
fn test_parse_redis_socket() {
    let config = config_from_file("tests/configs/redis_socket.php");
    assert_debug_equal(
        RedisConfig::Single(ConnectionInfo::from_str("redis+unix:///redis").unwrap()),
        config.redis,
    );
}

#[test]
fn test_parse_comment_whitespace() {
    let config = config_from_file("tests/configs/comment_whitespace.php");
    assert_eq!("https://cloud.example.com", config.nextcloud_url);
    assert_eq!("oc_", config.database_prefix);
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@127.0.0.1/nextcloud?ssl-mode=disabled",
        )
        .unwrap(),
        config.database,
    );
    assert_debug_equal(
        RedisConfig::Single(ConnectionInfo::from_str("redis://127.0.0.1").unwrap()),
        config.redis,
    );
}

#[test]
fn test_parse_port_in_host() {
    let config = config_from_file("tests/configs/port_in_host.php");
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@127.0.0.1:1234/nextcloud?ssl-mode=disabled",
        )
        .unwrap(),
        config.database,
    );
}

#[test]
fn test_parse_postgres_socket() {
    let config = config_from_file("tests/configs/postgres_socket.php");
    assert_debug_equal(
        AnyConnectOptions::from(
            PgConnectOptions::new()
                .socket("/var/run/postgresql")
                .username("redacted")
                .password("redacted")
                .database("nextcloud"),
        ),
        config.database,
    );
}

#[test]
fn test_parse_postgres_socket_folder() {
    let config = config_from_file("tests/configs/postgres_socket_folder.php");
    assert_debug_equal(
        AnyConnectOptions::from(
            PgConnectOptions::new()
                .socket("/var/run/postgresql")
                .username("redacted")
                .password("redacted")
                .database("nextcloud"),
        ),
        config.database,
    );
}

#[test]
fn test_parse_redis_cluster() {
    let config = config_from_file("tests/configs/redis.cluster.php");
    let mut conns = config.redis.into_vec();
    conns.sort_by(|a, b| a.addr.to_string().cmp(&b.addr.to_string()));
    assert_debug_equal(
        vec![
            ConnectionInfo::from_str("redis://:xxx@db1:6380").unwrap(),
            ConnectionInfo::from_str("redis://:xxx@db1:6381").unwrap(),
            ConnectionInfo::from_str("redis://:xxx@db1:6382").unwrap(),
            ConnectionInfo::from_str("redis://:xxx@db2:6380").unwrap(),
            ConnectionInfo::from_str("redis://:xxx@db2:6381").unwrap(),
            ConnectionInfo::from_str("redis://:xxx@db2:6382").unwrap(),
        ],
        conns,
    );
}

#[test]
fn test_parse_config_multiple() {
    let config = config_from_file("tests/configs/multiple/config.php");
    assert_eq!("https://cloud.example.com", config.nextcloud_url);
    assert_eq!("oc_", config.database_prefix);
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@127.0.0.1/nextcloud?ssl-mode=disabled",
        )
        .unwrap(),
        config.database,
    );
    assert_debug_equal(
        RedisConfig::Single(ConnectionInfo::from_str("redis://127.0.0.1").unwrap()),
        config.redis,
    );
}

#[test]
fn test_parse_config_mysql_fqdn() {
    let config = config_from_file("tests/configs/mysql_fqdn.php");
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@db.example.com/nextcloud?ssl-mode=preferred",
        )
        .unwrap(),
        config.database,
    );
}
