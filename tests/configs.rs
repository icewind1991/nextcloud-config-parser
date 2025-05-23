use nextcloud_config_parser::{
    parse, parse_glob, Config, Database, DbConnect, RedisClusterConnectionInfo, RedisConfig,
    RedisConnectionAddr, RedisConnectionInfo, RedisTlsParams, SslOptions,
};
use std::fmt::Debug;

use redis::{ConnectionAddr, ConnectionInfo};
use sqlx::mysql::{MySqlConnectOptions, MySqlSslMode};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{any::AnyConnectOptions, postgres::PgConnectOptions};
use std::str::FromStr;

#[cfg(test)]
#[track_caller]
fn assert_debug_equal<T: Debug>(a: T, b: T) {
    assert_eq!(format!("{:?}", a), format!("{:?}", b),);
}

#[cfg(test)]
fn config_from_file(path: &str) -> Config {
    parse(path).unwrap()
}

fn parse_redis(cfg: &str) -> RedisConnectionInfo {
    let redis = ConnectionInfo::from_str(cfg).unwrap();
    let addr = match redis.addr {
        ConnectionAddr::Tcp(host, port) => RedisConnectionAddr::Tcp {
            host,
            port,
            tls: false,
        },
        ConnectionAddr::TcpTls { host, port, .. } => RedisConnectionAddr::Tcp {
            host,
            port,
            tls: true,
        },
        ConnectionAddr::Unix(path) => RedisConnectionAddr::Unix { path },
    };
    RedisConnectionInfo {
        addr,
        db: redis.redis.db,
        username: redis.redis.username,
        password: redis.redis.password,
        tls_params: None,
    }
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

    assert_eq!(
        config.database.url(),
        "mysql://nextcloud:secret@127.0.0.1/nextcloud?ssl-mode=disabled"
    );

    assert_debug_equal(
        RedisConfig::Single(parse_redis("redis://127.0.0.1")),
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
        RedisConfig::Single(parse_redis("redis://127.0.0.1")),
        config.redis,
    );
}

#[test]
fn test_parse_full_redis() {
    let config = config_from_file("tests/configs/full_redis.php");
    assert_debug_equal(
        RedisConfig::Single(parse_redis("redis://name:moresecret@redis:1234/1")),
        config.redis,
    );
}

#[test]
fn test_parse_redis_socket() {
    let config = config_from_file("tests/configs/redis_socket.php");
    assert_debug_equal(
        RedisConfig::Single(parse_redis("redis+unix:///redis")),
        config.redis,
    );
}

#[test]
fn test_parse_redis_tls() {
    let config = config_from_file("tests/configs/redis_tls.php");
    assert_debug_equal(
        RedisConfig::Single(RedisConnectionInfo {
            addr: RedisConnectionAddr::Tcp {
                host: "127.0.0.1".into(),
                port: 6379,
                tls: true,
            },
            db: 0,
            username: None,
            password: None,
            tls_params: Some(RedisTlsParams {
                local_cert: Some("/certs/redis.crt".into()),
                local_pk: Some("/certs/redis.key".into()),
                ca_file: Some("/certs/ca.crt".into()),
                insecure: false,
                accept_invalid_hostname: false,
            }),
        }),
        config.redis,
    );
}

#[test]
fn test_parse_redis_cluster_tls() {
    let config = config_from_file("tests/configs/redis_cluster_tls.php");
    assert_debug_equal(
        RedisConfig::Cluster(RedisClusterConnectionInfo {
            addr: vec![
                RedisConnectionAddr::Tcp {
                    host: "db1".into(),
                    port: 6380,
                    tls: true,
                },
                RedisConnectionAddr::Tcp {
                    host: "db1".into(),
                    port: 6381,
                    tls: true,
                },
            ],
            db: 0,
            username: None,
            password: Some("xxx".into()),
            tls_params: Some(RedisTlsParams {
                local_cert: Some("/certs/redis.crt".into()),
                local_pk: Some("/certs/redis.key".into()),
                ca_file: Some("/certs/ca.crt".into()),
                insecure: false,
                accept_invalid_hostname: false,
            }),
        }),
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

    assert_eq!(
        config.database.url(),
        "mysql://nextcloud:secret@127.0.0.1/nextcloud?ssl-mode=disabled"
    );

    assert_debug_equal(
        RedisConfig::Single(parse_redis("redis://127.0.0.1")),
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

    assert_eq!(
        config.database.url(),
        "mysql://nextcloud:secret@127.0.0.1:1234/nextcloud?ssl-mode=disabled"
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

    assert_eq!(
        config.database.url(),
        "postgresql://redacted:redacted@localhost/nextcloud?host=/var/run/postgresql"
    );

    assert_debug_equal(
        PgConnectOptions::new()
            .socket("/var/run/postgresql")
            .host("localhost")
            .username("redacted")
            .password("redacted")
            .database("nextcloud"),
        PgConnectOptions::from_str(&config.database.url()).unwrap(),
    );
}

#[test]
fn test_parse_postgres_socket_empty_hostname() {
    let config = config_from_file("tests/configs/postgres_socket_no_host.php");
    assert_debug_equal(
        &Database::Postgres {
            database: "nextcloud".to_string(),
            username: "nextcloud".to_string(),
            password: "redacted".to_string(),
            connect: DbConnect::Socket("/run/postgresql".into()),
            ssl_options: SslOptions::Default,
        },
        &config.database,
    );

    assert_eq!(
        config.database.url(),
        "postgresql://nextcloud:redacted@localhost/nextcloud?host=/run/postgresql"
    );

    assert_debug_equal(
        PgConnectOptions::new()
            .socket("/run/postgresql")
            .host("localhost")
            .username("nextcloud")
            .password("redacted")
            .database("nextcloud"),
        PgConnectOptions::from_str(&config.database.url()).unwrap(),
    );
}

#[test]
fn test_parse_postgres_socket_no_pass() {
    let config = config_from_file("tests/configs/postgres_socket_no_pass.php");
    assert_debug_equal(
        &Database::Postgres {
            database: "nextcloud".to_string(),
            username: "redacted".to_string(),
            password: "".to_string(),
            connect: DbConnect::Socket("/var/run/postgresql".into()),
            ssl_options: SslOptions::Default,
        },
        &config.database,
    );

    assert_eq!(
        config.database.url(),
        "postgresql://redacted:@localhost/nextcloud?host=/var/run/postgresql"
    );
    assert_debug_equal(
        PgConnectOptions::new()
            .socket("/var/run/postgresql")
            .host("localhost")
            .username("redacted")
            .database("nextcloud"),
        PgConnectOptions::from_str(&config.database.url()).unwrap(),
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

    assert_eq!(
        config.database.url(),
        "postgresql://redacted:redacted@localhost/nextcloud?host=/var/run/postgresql"
    );

    assert_debug_equal(
        PgConnectOptions::new()
            .socket("/var/run/postgresql")
            .host("localhost")
            .username("redacted")
            .password("redacted")
            .database("nextcloud"),
        PgConnectOptions::from_str(&config.database.url()).unwrap(),
    );
}

#[test]
fn test_parse_redis_cluster() {
    let config = config_from_file("tests/configs/redis.cluster.php");
    let mut addresses = config.redis.addr().cloned().collect::<Vec<_>>();
    addresses.sort_by(|a, b| format!("{:?}", a).cmp(&format!("{:?}", b)));
    assert_debug_equal(
        vec![
            parse_redis("redis://:xxx@db1:6380").addr,
            parse_redis("redis://:xxx@db1:6381").addr,
            parse_redis("redis://:xxx@db1:6382").addr,
            parse_redis("redis://:xxx@db2:6380").addr,
            parse_redis("redis://:xxx@db2:6381").addr,
            parse_redis("redis://:xxx@db2:6382").addr,
        ],
        addresses,
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

    assert_eq!(
        config.database.url(),
        "mysql://nextcloud:secret@127.0.0.1/nextcloud?ssl-mode=disabled"
    );

    assert_debug_equal(
        RedisConfig::Single(parse_redis("redis://127.0.0.1")),
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
    assert_debug_equal(
        AnyConnectOptions::from_str("sqlite:///nc/nextcloud.db").unwrap(),
        AnyConnectOptions::from_str(&config.database.url()).unwrap(),
    );
    assert_debug_equal(
        RedisConfig::Single(parse_redis("redis://127.0.0.1")),
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

    assert_eq!(
        config.database.url(),
        "mysql://nextcloud:secret@db.example.com/nextcloud"
    );

    assert_debug_equal(
        MySqlConnectOptions::new()
            .username("nextcloud")
            .password("secret")
            .database("nextcloud")
            .host("db.example.com")
            .ssl_mode(MySqlSslMode::Preferred),
        MySqlConnectOptions::from_str(&config.database.url()).unwrap(),
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

    assert_eq!(
        config.database.url(),
        "mysql://nextcloud:secret@1.2.3.4/nextcloud"
    );

    assert_debug_equal(
        MySqlConnectOptions::new()
            .username("nextcloud")
            .password("secret")
            .database("nextcloud")
            .host("1.2.3.4")
            .ssl_mode(MySqlSslMode::Preferred),
        MySqlConnectOptions::from_str(&config.database.url()).unwrap(),
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

    assert_eq!(
        config.database.url(),
        "mysql://nextcloud:secret@db.example.com/nextcloud?ssl-mode=verify_identity&ssl-ca=/ca-cert.pem"
    );

    assert_debug_equal(
        MySqlConnectOptions::new()
            .username("nextcloud")
            .password("secret")
            .database("nextcloud")
            .host("db.example.com")
            .ssl_mode(MySqlSslMode::VerifyIdentity)
            .ssl_ca("/ca-cert.pem"),
        MySqlConnectOptions::from_str(&config.database.url()).unwrap(),
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
    assert_eq!(
        config.database.url(),
        "mysql://nextcloud:secret@db.example.com/nextcloud?ssl-mode=verify_ca&ssl-ca=/ca-cert.pem"
    );

    assert_debug_equal(
        MySqlConnectOptions::new()
            .username("nextcloud")
            .password("secret")
            .database("nextcloud")
            .host("db.example.com")
            .ssl_mode(MySqlSslMode::VerifyCa)
            .ssl_ca("/ca-cert.pem"),
        MySqlConnectOptions::from_str(&config.database.url()).unwrap(),
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
    assert_eq!(
        config.database.url(),
        "postgresql://redacted:redacted@1.2.3.4/nextcloud?sslmode=disable"
    );

    assert_debug_equal(
        PgConnectOptions::new()
            .host("1.2.3.4")
            .username("redacted")
            .password("redacted")
            .database("nextcloud")
            .port(5432)
            .ssl_mode(sqlx::postgres::PgSslMode::Disable),
        PgConnectOptions::from_str(&config.database.url()).unwrap(),
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
    assert_eq!(
        config.database.url(),
        "postgresql://redacted:redacted@pg.example.com/nextcloud"
    );

    assert_debug_equal(
        PgConnectOptions::new()
            .host("pg.example.com")
            .username("redacted")
            .password("redacted")
            .port(5432)
            .database("nextcloud"),
        PgConnectOptions::from_str(&config.database.url()).unwrap(),
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

    assert_debug_equal(
        SqliteConnectOptions::new().filename("/nc/data/owncloud.db"),
        SqliteConnectOptions::from_str(&config.database.url()).unwrap(),
    );
}

#[test]
fn test_parse_config_nested_array() {
    let config = config_from_file("tests/configs/nested_array.php");
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
}

#[test]
fn test_parse_postgres_escaped_credentials() {
    let config = config_from_file("tests/configs/postgres_escape.php");
    assert_debug_equal(
        &Database::Postgres {
            database: "nextcloud".to_string(),
            username: "reda:cted".to_string(),
            password: "reda@cted".to_string(),
            connect: DbConnect::Tcp {
                host: "1.2.3.4".to_string(),
                port: 5432,
            },
            ssl_options: SslOptions::Disabled,
        },
        &config.database,
    );
    assert_eq!(
        config.database.url(),
        "postgresql://reda%3Acted:reda%40cted@1.2.3.4/nextcloud?sslmode=disable"
    );

    assert_debug_equal(
        PgConnectOptions::new()
            .host("1.2.3.4")
            .username("reda:cted")
            .password("reda@cted")
            .port(5432)
            .database("nextcloud")
            .ssl_mode(sqlx::postgres::PgSslMode::Disable),
        PgConnectOptions::from_str(&config.database.url()).unwrap(),
    );
}
