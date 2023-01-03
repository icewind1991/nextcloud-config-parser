mod nc;
mod php;

use miette::Diagnostic;
#[cfg(feature = "redis-connect")]
use redis::{ConnectionAddr, ConnectionInfo};
use std::fmt::Debug;
#[cfg(feature = "redis-connect")]
use std::iter::once;
use std::path::PathBuf;
use thiserror::Error;

pub use nc::{parse, parse_glob};

#[derive(Debug)]
pub struct Config {
    pub database: Database,
    pub database_prefix: String,
    #[cfg(feature = "redis-connect")]
    pub redis: RedisConfig,
    pub nextcloud_url: String,
}

#[cfg(feature = "redis-connect")]
#[derive(Debug)]
pub enum RedisConfig {
    Single(ConnectionInfo),
    Cluster(Vec<ConnectionInfo>),
}

#[cfg(feature = "redis-connect")]
impl RedisConfig {
    pub fn addr(&self) -> impl Iterator<Item = &ConnectionAddr> {
        let boxed: Box<dyn Iterator<Item = &ConnectionAddr>> = match self {
            RedisConfig::Single(conn) => Box::new(once(&conn.addr)),
            RedisConfig::Cluster(conns) => Box::new(conns.iter().map(|conn| &conn.addr)),
        };
        boxed
    }

    pub fn db(&self) -> i64 {
        match self {
            RedisConfig::Single(conn) => conn.redis.db,
            RedisConfig::Cluster(conns) => {
                conns.first().map(|conn| conn.redis.db).unwrap_or_default()
            }
        }
    }

    pub fn username(&self) -> Option<&str> {
        match self {
            RedisConfig::Single(conn) => conn.redis.username.as_deref(),
            RedisConfig::Cluster(conns) => conns
                .first()
                .map(|conn| conn.redis.username.as_deref())
                .unwrap_or_default(),
        }
    }

    pub fn passwd(&self) -> Option<&str> {
        match self {
            RedisConfig::Single(conn) => conn.redis.password.as_deref(),
            RedisConfig::Cluster(conns) => conns
                .first()
                .map(|conn| conn.redis.password.as_deref())
                .unwrap_or_default(),
        }
    }

    pub fn into_vec(self) -> Vec<ConnectionInfo> {
        match self {
            RedisConfig::Single(conn) => vec![conn],
            RedisConfig::Cluster(vec) => vec,
        }
    }
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error, Diagnostic)]
pub enum Error {
    #[error(transparent)]
    #[diagnostic(transparent)]
    Php(PhpParseError),
    #[error("Provided config file doesn't seem to be a nextcloud config file: {0:#}")]
    NotAConfig(#[from] NotAConfigError),
    #[error("Failed to read config file")]
    ReadFailed(std::io::Error, PathBuf),
    #[error("invalid database configuration: {0}")]
    InvalidDb(#[from] DbError),
    #[error("Invalid redis configuration")]
    Redis,
    #[error("`overwrite.cli.url` not set`")]
    NoUrl,
    #[error("Failed to execute php to parse configuration")]
    Exec,
}

#[derive(Debug, Error, Diagnostic)]
#[error("Error while parsing '{path}':\n{err}")]
#[diagnostic(forward(err))]
pub struct PhpParseError {
    err: php_literal_parser::ParseError,
    path: PathBuf,
}

#[derive(Debug, Error)]
pub enum DbError {
    #[error("unsupported database type {0}")]
    Unsupported(String),
    #[error("no username set")]
    NoUsername,
    #[error("no password set")]
    NoPassword,
    #[error("no data directory")]
    NoDataDirectory,
}

#[derive(Debug, Error)]
pub enum NotAConfigError {
    #[error("$CONFIG not found in file")]
    NoConfig(PathBuf),
    #[error("$CONFIG is not an array")]
    NotAnArray(PathBuf),
}

#[derive(Debug)]
pub enum SslOptions {
    Enabled {
        key: String,
        cert: String,
        ca: String,
        verify: bool,
    },
    Disabled,
    Default,
}

#[derive(Debug)]
pub enum Database {
    Sqlite {
        database: PathBuf,
    },
    MySql {
        database: String,
        username: String,
        password: String,
        connect: DbConnect,
        ssl_options: SslOptions,
    },
    Postgres {
        database: String,
        username: String,
        password: String,
        connect: DbConnect,
        ssl_options: SslOptions,
    },
}

#[derive(Debug)]
pub enum DbConnect {
    Tcp { host: String, port: u16 },
    Socket(PathBuf),
}

#[cfg(feature = "db-sqlx")]
impl From<Database> for sqlx::any::AnyConnectOptions {
    fn from(cfg: Database) -> Self {
        use sqlx::{
            mysql::{MySqlConnectOptions, MySqlSslMode},
            postgres::{PgConnectOptions, PgSslMode},
            sqlite::SqliteConnectOptions,
        };

        match cfg {
            Database::Sqlite { database } => {
                SqliteConnectOptions::default().filename(database).into()
            }
            Database::MySql {
                database,
                username,
                password,
                connect,
                ssl_options,
            } => {
                let mut options = MySqlConnectOptions::default()
                    .database(&database)
                    .username(&username)
                    .password(&password);
                match ssl_options {
                    SslOptions::Enabled { ca, verify, .. } => {
                        options = options.ssl_ca(ca);
                        options = options.ssl_mode(if verify {
                            MySqlSslMode::VerifyIdentity
                        } else {
                            MySqlSslMode::VerifyCa
                        });
                    }
                    SslOptions::Disabled => {
                        options = options.ssl_mode(MySqlSslMode::Disabled);
                    }
                    SslOptions::Default => {}
                }
                match connect {
                    DbConnect::Socket(socket) => {
                        options = options.socket(socket);
                    }
                    DbConnect::Tcp { host, port } => {
                        options = options.host(&host).port(port);
                    }
                }
                options.into()
            }
            Database::Postgres {
                database,
                username,
                password,
                connect,
                ssl_options,
            } => {
                let mut options = PgConnectOptions::default()
                    .database(&database)
                    .username(&username)
                    .password(&password);
                if matches!(ssl_options, SslOptions::Disabled) {
                    options = options.ssl_mode(PgSslMode::Disable);
                }
                match connect {
                    DbConnect::Socket(socket) => {
                        options = options.socket(socket);
                    }
                    DbConnect::Tcp { host, port } => {
                        options = options.host(&host).port(port);
                    }
                }
                options.into()
            }
        }
    }
}

#[cfg(test)]
#[track_caller]
pub fn assert_debug_equal<T: Debug>(a: T, b: T) {
    assert_eq!(format!("{:?}", a), format!("{:?}", b),);
}
