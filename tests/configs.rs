use nextcloud_config_parser::{parse, parse_glob, Config, Database, DbConnect, SslOptions};
use std::fmt::Debug;

#[cfg(feature = "redis-connect")]
use nextcloud_config_parser::RedisConfig;
#[cfg(feature = "redis-connect")]
use redis::ConnectionInfo;
#[cfg(feature = "db-sqlx")]
use sqlx::{any::AnyConnectOptions, postgres::PgConnectOptions};
#[cfg(feature = "db-sqlx")]
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
        RedisConfig::Single(
            ConnectionInfo::from_str("redis://name:moresecret@redis:1234/1").unwrap(),
        ),
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
    #[cfg(feature = "db-sqlx")]
    assert_debug_equal(
        AnyConnectOptions::from(
            PgConnectOptions::new()
                .socket("/var/run/postgresql")
                .username("redacted")
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
