use crate::{
    split_host, Config, Database, DbConnect, DbError, Error, NotAConfigError, PhpParseError,
    RedisClusterConnectionInfo, RedisConnectionInfo, RedisTlsParams, Result, SslOptions,
};
use crate::{RedisConfig, RedisConnectionAddr};
use php_literal_parser::Value;
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
    let redis = parse_redis_options(&parsed);

    Ok(Config {
        database,
        database_prefix,
        nextcloud_url,
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
            let password = parsed["dbpassword"].as_str().unwrap_or_default();
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

enum RedisAddress {
    Single(RedisConnectionAddr),
    Cluster(Vec<RedisConnectionAddr>),
}

fn parse_redis_options(parsed: &Value) -> RedisConfig {
    let (redis_options, address) = if parsed["redis.cluster"].is_array() {
        let redis_options = &parsed["redis.cluster"];
        let seeds = redis_options["seeds"].values();
        let mut addresses = seeds
            .filter_map(|seed| seed.as_str())
            .map(|seed| {
                RedisConnectionAddr::parse(seed, None, redis_options["ssl_context"].is_array())
            })
            .collect::<Vec<_>>();
        addresses.sort();
        (redis_options, RedisAddress::Cluster(addresses))
    } else {
        let redis_options = &parsed["redis"];
        let host = redis_options["host"].as_str().unwrap_or("127.0.0.1");
        let address = RedisAddress::Single(RedisConnectionAddr::parse(
            host,
            redis_options["port"]
                .as_int()
                .and_then(|port| u16::try_from(port).ok()),
            redis_options["ssl_context"].is_array(),
        ));
        (redis_options, address)
    };

    let tls_params = if redis_options["ssl_context"].is_array() {
        let ssl_options = &redis_options["ssl_context"];
        Some(RedisTlsParams {
            local_cert: ssl_options["local_cert"].as_str().map(From::from),
            local_pk: ssl_options["local_pk"].as_str().map(From::from),
            ca_file: ssl_options["cafile"].as_str().map(From::from),
            accept_invalid_hostname: ssl_options["verify_peer_name"] == false,
            insecure: ssl_options["verify_peer "] == false,
        })
    } else {
        None
    };

    let db = redis_options["dbindex"].clone().into_int().unwrap_or(0);
    let password = redis_options["password"]
        .as_str()
        .filter(|pass| !pass.is_empty())
        .map(String::from);
    let username = redis_options["user"]
        .as_str()
        .filter(|user| !user.is_empty())
        .map(String::from);

    match address {
        RedisAddress::Single(addr) => RedisConfig::Single(RedisConnectionInfo {
            addr,
            db,
            username,
            password,
            tls_params,
        }),
        RedisAddress::Cluster(addr) => RedisConfig::Cluster(RedisClusterConnectionInfo {
            addr,
            db,
            username,
            password,
            tls_params,
        }),
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
