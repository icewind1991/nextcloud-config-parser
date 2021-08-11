#[cfg(feature = "redis-connect")]
use crate::RedisConfig;
use crate::{
    Config, Database, DbConnect, DbError, Error, NotAConfigError, PhpParseError, Result, SslOptions,
};
use php_literal_parser::Value;
#[cfg(feature = "redis-connect")]
use redis::{ConnectionAddr, ConnectionInfo, RedisConnectionInfo};
use std::collections::HashMap;
use std::fs::DirEntry;
use std::iter::once;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

static CONFIG_CONSTANTS: &[(&str, &str)] = &[
    (r"\RedisCluster::FAILOVER_NONE", "0"),
    (r"\RedisCluster::FAILOVER_ERROR", "1"),
    (r"\RedisCluster::DISTRIBUTE", "2"),
    (r"\RedisCluster::FAILOVER_DISTRIBUTE_SLAVES", "3"),
    (r"\PDO::MYSQL_ATTR_SSL_KEY", "1007"),
    (r"\PDO::MYSQL_ATTR_SSL_CERT", "1008"),
    (r"\PDO::MYSQL_ATTR_SSL_CA", "1009"),
    (r"\PDO::MYSQL_ATTR_SSL_VERIFY_SERVER_CERT", "1014"),
];

fn glob_config_files(path: impl AsRef<Path>) -> impl Iterator<Item = PathBuf> {
    let main: PathBuf = path.as_ref().into();
    let files = if let Some(parent) = path.as_ref().parent() {
        if let Ok(dir) = parent.read_dir() {
            Some(dir.filter_map(Result::ok).filter_map(|file: DirEntry| {
                let path = file.path();
                match path.to_str() {
                    Some(path_str) if path_str.ends_with(".config.php") => Some(path),
                    _ => None,
                }
            }))
        } else {
            None
        }
    } else {
        None
    };

    once(main).chain(files.into_iter().flatten())
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
    php_literal_parser::from_str(php).map_err(|err| {
        Error::Php(PhpParseError {
            err,
            path: path.as_ref().into(),
            source: php.into(),
        })
    })
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

fn parse_files(files: impl IntoIterator<Item = PathBuf>) -> Result<Config> {
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
    #[cfg(feature = "redis-connect")]
    let redis = parse_redis_options(&parsed);

    Ok(Config {
        database,
        database_prefix,
        nextcloud_url,
        #[cfg(feature = "redis-connect")]
        redis,
    })
}

pub fn parse(path: impl AsRef<Path>) -> Result<Config> {
    parse_files(once(path.as_ref().into()))
}

pub fn parse_glob(path: impl AsRef<Path>) -> Result<Config> {
    parse_files(glob_config_files(path))
}

fn parse_db_options(parsed: &Value) -> Result<Database> {
    match parsed["dbtype"].as_str() {
        Some("mysql") => {
            let username = parsed["dbuser"].as_str().ok_or(DbError::NoUsername)?;
            let password = parsed["dbpassword"].as_str().ok_or(DbError::NoPassword)?;
            let socket_addr1 = PathBuf::from("/var/run/mysqld/mysqld.sock");
            let socket_addr2 = PathBuf::from("/tmp/mysql.sock");
            let socket_addr3 = PathBuf::from("/run/mysql/mysql.sock");
            let (mut connect, disable_ssl) =
                match split_host(parsed["dbhost"].as_str().unwrap_or_default()) {
                    ("localhost", None, None) if socket_addr1.exists() => {
                        (DbConnect::Socket(socket_addr1), false)
                    }
                    ("localhost", None, None) if socket_addr2.exists() => {
                        (DbConnect::Socket(socket_addr2), false)
                    }
                    ("localhost", None, None) if socket_addr3.exists() => {
                        (DbConnect::Socket(socket_addr3), false)
                    }
                    (addr, None, None) => (
                        DbConnect::Tcp {
                            host: addr.into(),
                            port: 3306,
                        },
                        IpAddr::from_str(addr).is_ok(),
                    ),
                    (addr, Some(port), None) => (
                        DbConnect::Tcp {
                            host: addr.into(),
                            port,
                        },
                        IpAddr::from_str(addr).is_ok(),
                    ),
                    (_, None, Some(socket)) => (DbConnect::Socket(socket.into()), false),
                    (_, Some(_), Some(_)) => {
                        unreachable!()
                    }
                };
            if let Some(port) = parsed["dbport"].clone().into_int() {
                if let DbConnect::Tcp {
                    port: connect_port, ..
                } = &mut connect
                {
                    *connect_port = port as u16;
                }
            }
            let database = parsed["dbname"].as_str().unwrap_or("owncloud");

            let verify = parsed["dbdriveroptions"][1014] // MYSQL_ATTR_SSL_VERIFY_SERVER_CERT
                .clone()
                .into_bool()
                .unwrap_or(true);

            let ssl_options = if let (Some(ssl_key), Some(ssl_cert), Some(ssl_ca)) = (
                parsed["dbdriveroptions"][1007].as_str(), // MYSQL_ATTR_SSL_KEY
                parsed["dbdriveroptions"][1008].as_str(), // MYSQL_ATTR_SSL_CERT
                parsed["dbdriveroptions"][1009].as_str(), // MYSQL_ATTR_SSL_CA
            ) {
                SslOptions::Enabled {
                    key: ssl_key.into(),
                    cert: ssl_cert.into(),
                    ca: ssl_ca.into(),
                    verify,
                }
                // if MYSQL_ATTR_SSL_VERIFY_SERVER_CERT is disabled, we should be able to use ssl even with raw ip
            } else if disable_ssl && verify {
                SslOptions::Disabled
            } else {
                SslOptions::Default
            };

            Ok(Database::MySql {
                database: database.into(),
                username: username.into(),
                password: password.into(),
                connect,
                ssl_options,
            })
        }
        Some("pgsql") => {
            let username = parsed["dbuser"].as_str().ok_or(DbError::NoUsername)?;
            let password = parsed["dbpassword"].as_str().ok_or(DbError::NoPassword)?;
            let (mut connect, disable_ssl) =
                match split_host(parsed["dbhost"].as_str().unwrap_or_default()) {
                    (addr, None, None) => (
                        DbConnect::Tcp {
                            host: addr.into(),
                            port: 5432,
                        },
                        IpAddr::from_str(addr).is_ok(),
                    ),
                    (addr, Some(port), None) => (
                        DbConnect::Tcp {
                            host: addr.into(),
                            port,
                        },
                        IpAddr::from_str(addr).is_ok(),
                    ),
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
                        (DbConnect::Socket(socket_path.into()), false)
                    }
                    (_, Some(_), Some(_)) => {
                        unreachable!()
                    }
                };
            if let Some(port) = parsed["dbport"].clone().into_int() {
                if let DbConnect::Tcp {
                    port: connect_port, ..
                } = &mut connect
                {
                    *connect_port = port as u16;
                }
            }
            let database = parsed["dbname"].as_str().unwrap_or("owncloud");

            let ssl_options = if disable_ssl {
                SslOptions::Disabled
            } else {
                SslOptions::Default
            };

            Ok(Database::Postgres {
                database: database.into(),
                username: username.into(),
                password: password.into(),
                connect,
                ssl_options,
            })
        }
        Some("sqlite3") | Some("sqlite") | None => {
            let data_dir = parsed["datadirectory"]
                .as_str()
                .ok_or(DbError::NoDataDirectory)?;
            let db_name = parsed["dbname"].as_str().unwrap_or("owncloud");
            Ok(Database::Sqlite {
                database: format!("{}/{}.db", data_dir, db_name).into(),
            })
        }
        Some(ty) => Err(Error::InvalidDb(DbError::Unsupported(ty.into()))),
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

#[cfg(feature = "redis-connect")]
enum RedisAddress {
    Single(ConnectionAddr),
    Cluster(Vec<ConnectionAddr>),
}

#[cfg(feature = "redis-connect")]
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
    let password = redis_options["password"]
        .as_str()
        .filter(|pass| !pass.is_empty())
        .map(String::from);

    match address {
        RedisAddress::Single(addr) => RedisConfig::Single(ConnectionInfo {
            addr,
            redis: RedisConnectionInfo {
                db,
                username: None,
                password,
            },
        }),
        RedisAddress::Cluster(addresses) => RedisConfig::Cluster(
            addresses
                .into_iter()
                .map(|addr| ConnectionInfo {
                    addr,
                    redis: RedisConnectionInfo {
                        db,
                        username: None,
                        password: password.clone(),
                    },
                })
                .collect(),
        ),
    }
}

#[test]
#[cfg(feature = "redis-connect")]
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
fn assert_debug_equal<T: Debug>(a: T, b: T) {
    assert_eq!(format!("{:?}", a), format!("{:?}", b),);
}

#[cfg(test)]
#[allow(unused_imports)]
use sqlx::{any::AnyConnectOptions, postgres::PgConnectOptions};
#[cfg(test)]
use std::fmt::Debug;

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
        &Database::MySql {
            database: "nextcloud".to_string(),
            username: "nextcloud".to_string(),
            password: "secret".to_string(),
            connect: DbConnect::Tcp {
                host: "127.0.0.1".to_string(),
                port: 3306,
            },
            ssl_options: SslOptions::Disabled,
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@127.0.0.1/nextcloud?ssl-mode=disabled",
        )
        .unwrap(),
        config.database.into(),
    );
    #[cfg(feature = "redis-connect")]
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
#[cfg(feature = "redis-connect")]
fn test_parse_empty_redis_password() {
    let config = config_from_file("tests/configs/empty_redis_password.php");
    assert_debug_equal(
        RedisConfig::Single(ConnectionInfo::from_str("redis://127.0.0.1").unwrap()),
        config.redis,
    );
}

#[test]
#[cfg(feature = "redis-connect")]
fn test_parse_full_redis() {
    let config = config_from_file("tests/configs/full_redis.php");
    assert_debug_equal(
        RedisConfig::Single(ConnectionInfo::from_str("redis://:moresecret@redis:1234/1").unwrap()),
        config.redis,
    );
}

#[test]
#[cfg(feature = "redis-connect")]
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
        &Database::MySql {
            database: "nextcloud".to_string(),
            username: "nextcloud".to_string(),
            password: "secret".to_string(),
            connect: DbConnect::Tcp {
                host: "127.0.0.1".to_string(),
                port: 3306,
            },
            ssl_options: SslOptions::Disabled,
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@127.0.0.1/nextcloud?ssl-mode=disabled",
        )
        .unwrap(),
        config.database.into(),
    );
    #[cfg(feature = "redis-connect")]
    assert_debug_equal(
        RedisConfig::Single(ConnectionInfo::from_str("redis://127.0.0.1").unwrap()),
        config.redis,
    );
}

#[test]
fn test_parse_port_in_host() {
    let config = config_from_file("tests/configs/port_in_host.php");
    assert_debug_equal(
        &Database::MySql {
            database: "nextcloud".to_string(),
            username: "nextcloud".to_string(),
            password: "secret".to_string(),
            connect: DbConnect::Tcp {
                host: "127.0.0.1".to_string(),
                port: 1234,
            },
            ssl_options: SslOptions::Disabled,
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@127.0.0.1:1234/nextcloud?ssl-mode=disabled",
        )
        .unwrap(),
        config.database.into(),
    );
}

#[test]
fn test_parse_postgres_socket() {
    let config = config_from_file("tests/configs/postgres_socket.php");
    assert_debug_equal(
        &Database::Postgres {
            database: "nextcloud".to_string(),
            username: "redacted".to_string(),
            password: "redacted".to_string(),
            connect: DbConnect::Socket("/var/run/postgresql".into()),
            ssl_options: SslOptions::Default,
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from(
            PgConnectOptions::new()
                .socket("/var/run/postgresql")
                .username("redacted")
                .password("redacted")
                .database("nextcloud"),
        ),
        config.database.into(),
    );
}

#[test]
fn test_parse_postgres_socket_folder() {
    let config = config_from_file("tests/configs/postgres_socket_folder.php");
    assert_debug_equal(
        &Database::Postgres {
            database: "nextcloud".to_string(),
            username: "redacted".to_string(),
            password: "redacted".to_string(),
            connect: DbConnect::Socket("/var/run/postgresql".into()),
            ssl_options: SslOptions::Default,
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from(
            PgConnectOptions::new()
                .socket("/var/run/postgresql")
                .username("redacted")
                .password("redacted")
                .database("nextcloud"),
        ),
        config.database.into(),
    );
}

#[test]
#[cfg(feature = "redis-connect")]
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
    let config = parse_glob("tests/configs/multiple/config.php").unwrap();
    assert_eq!("https://cloud.example.com", config.nextcloud_url);
    assert_eq!("oc_", config.database_prefix);
    assert_debug_equal(
        &Database::MySql {
            database: "nextcloud".to_string(),
            username: "nextcloud".to_string(),
            password: "secret".to_string(),
            connect: DbConnect::Tcp {
                host: "127.0.0.1".to_string(),
                port: 3306,
            },
            ssl_options: SslOptions::Disabled,
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@127.0.0.1/nextcloud?ssl-mode=disabled",
        )
        .unwrap(),
        config.database.into(),
    );
    #[cfg(feature = "redis-connect")]
    assert_debug_equal(
        RedisConfig::Single(ConnectionInfo::from_str("redis://127.0.0.1").unwrap()),
        config.redis,
    );
}

#[test]
fn test_parse_config_multiple_no_glob() {
    let config = config_from_file("tests/configs/multiple/config.php");
    assert_eq!("https://cloud.example.com", config.nextcloud_url);
    assert_eq!("oc_", config.database_prefix);
    assert_debug_equal(
        &Database::Sqlite {
            database: "/nc/nextcloud.db".into(),
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from_str("sqlite:///nc/nextcloud.db").unwrap(),
        config.database.into(),
    );
    #[cfg(feature = "redis-connect")]
    assert_debug_equal(
        RedisConfig::Single(ConnectionInfo::from_str("redis://127.0.0.1").unwrap()),
        config.redis,
    );
}

#[test]
fn test_parse_config_mysql_fqdn() {
    let config = config_from_file("tests/configs/mysql_fqdn.php");
    assert_debug_equal(
        &Database::MySql {
            database: "nextcloud".to_string(),
            username: "nextcloud".to_string(),
            password: "secret".to_string(),
            connect: DbConnect::Tcp {
                host: "db.example.com".to_string(),
                port: 3306,
            },
            ssl_options: SslOptions::Default,
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@db.example.com/nextcloud?ssl-mode=preferred",
        )
        .unwrap(),
        config.database.into(),
    );
}

#[test]
fn test_parse_config_mysql_ip_no_verify() {
    let config = config_from_file("tests/configs/mysql_ip_no_verify.php");
    assert_debug_equal(
        &Database::MySql {
            database: "nextcloud".to_string(),
            username: "nextcloud".to_string(),
            password: "secret".to_string(),
            connect: DbConnect::Tcp {
                host: "1.2.3.4".to_string(),
                port: 3306,
            },
            ssl_options: SslOptions::Default,
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@1.2.3.4/nextcloud?ssl-mode=preferred",
        )
        .unwrap(),
        config.database.into(),
    );
}

#[test]
fn test_parse_config_mysql_ssl_ca() {
    let config = config_from_file("tests/configs/mysql_ssl_ca.php");
    assert_debug_equal(
        &Database::MySql {
            database: "nextcloud".to_string(),
            username: "nextcloud".to_string(),
            password: "secret".to_string(),
            connect: DbConnect::Tcp {
                host: "db.example.com".to_string(),
                port: 3306,
            },
            ssl_options: SslOptions::Enabled {
                key: "/ssl-key.pem".into(),
                cert: "/ssl-cert.pem".into(),
                ca: "/ca-cert.pem".into(),
                verify: true,
            },
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@db.example.com/nextcloud?ssl-mode=verify_identity&ssl-ca=/ca-cert.pem",
        )
        .unwrap(),
        config.database.into(),
    );
}

#[test]
fn test_parse_config_mysql_ssl_ca_no_verify() {
    let config = config_from_file("tests/configs/mysql_ssl_ca_no_verify.php");
    assert_debug_equal(
        &Database::MySql {
            database: "nextcloud".to_string(),
            username: "nextcloud".to_string(),
            password: "secret".to_string(),
            connect: DbConnect::Tcp {
                host: "db.example.com".to_string(),
                port: 3306,
            },
            ssl_options: SslOptions::Enabled {
                key: "/ssl-key.pem".into(),
                cert: "/ssl-cert.pem".into(),
                ca: "/ca-cert.pem".into(),
                verify: false,
            },
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
        assert_debug_equal(
        AnyConnectOptions::from_str(
            "mysql://nextcloud:secret@db.example.com/nextcloud?ssl-mode=verify_ca&ssl-ca=/ca-cert.pem",
        )
            .unwrap(),
        config.database.into(),
    );
}

#[test]
fn test_parse_postgres_ip() {
    let config = config_from_file("tests/configs/postgres_ip.php");
    assert_debug_equal(
        &Database::Postgres {
            database: "nextcloud".to_string(),
            username: "redacted".to_string(),
            password: "redacted".to_string(),
            connect: DbConnect::Tcp {
                host: "1.2.3.4".to_string(),
                port: 5432,
            },
            ssl_options: SslOptions::Disabled,
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from(
            PgConnectOptions::new()
                .host("1.2.3.4")
                .username("redacted")
                .password("redacted")
                .database("nextcloud")
                .ssl_mode(sqlx::postgres::PgSslMode::Disable),
        ),
        config.database.into(),
    );
}

#[test]
fn test_parse_postgres_fqdn() {
    let config = config_from_file("tests/configs/postgres_fqdn.php");
    assert_debug_equal(
        &Database::Postgres {
            database: "nextcloud".to_string(),
            username: "redacted".to_string(),
            password: "redacted".to_string(),
            connect: DbConnect::Tcp {
                host: "pg.example.com".to_string(),
                port: 5432,
            },
            ssl_options: SslOptions::Default,
        },
        &config.database,
    );
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from(
            PgConnectOptions::new()
                .host("pg.example.com")
                .username("redacted")
                .password("redacted")
                .database("nextcloud"),
        ),
        config.database.into(),
    );
}

#[test]
fn test_parse_config_sqlite_default_db() {
    let config = config_from_file("tests/configs/sqlite_default_db.php");
    assert_debug_equal(
        &Database::Sqlite {
            database: "/nc/data/owncloud.db".into(),
        },
        &config.database,
    );
}
