mod nc;

use form_urlencoded::Serializer;
use itertools::Either;
use miette::Diagnostic;
use std::iter::once;
use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;

pub use nc::{parse, parse_glob};

#[derive(Debug)]
pub struct Config {
    pub database: Database,
    pub database_prefix: String,
    pub redis: RedisConfig,
    pub nextcloud_url: String,
}

#[derive(Debug)]
pub enum RedisConfig {
    Single(RedisConnectionInfo),
    Cluster(RedisClusterConnectionInfo),
}

impl RedisConfig {
    pub fn as_single(&self) -> Option<RedisConnectionInfo> {
        match self {
            RedisConfig::Single(single) => Some(single.clone()),
            RedisConfig::Cluster(cluster) => cluster.iter().next(),
        }
    }
}

#[derive(Clone, Debug, PartialOrd, PartialEq, Ord, Eq)]
pub enum RedisConnectionAddr {
    Tcp { host: String, port: u16, tls: bool },
    Unix { path: PathBuf },
}

impl RedisConnectionAddr {
    fn parse(mut host: &str, port: Option<u16>, tls: bool) -> Self {
        if host.starts_with("/") {
            RedisConnectionAddr::Unix { path: host.into() }
        } else {
            let tls = if host.starts_with("tls://") || host.starts_with("rediss://") {
                host = host.split_once("://").unwrap().1;
                true
            } else {
                tls
            };
            if host == "localhost" {
                host = "127.0.0.1";
            }
            let (host, port, _) = if let Some(port) = port {
                (host, Some(port), None)
            } else {
                split_host(host)
            };
            RedisConnectionAddr::Tcp {
                host: host.into(),
                port: port.unwrap_or(6379),
                tls,
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct RedisClusterConnectionInfo {
    pub addr: Vec<RedisConnectionAddr>,
    pub db: i64,
    pub username: Option<String>,
    pub password: Option<String>,
    pub tls_params: Option<RedisTlsParams>,
}

impl RedisClusterConnectionInfo {
    pub fn iter(&self) -> impl Iterator<Item = RedisConnectionInfo> + '_ {
        self.addr.iter().cloned().map(|addr| RedisConnectionInfo {
            addr,
            db: self.db,
            username: self.username.clone(),
            password: self.password.clone(),
            tls_params: self.tls_params.clone(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct RedisConnectionInfo {
    pub addr: RedisConnectionAddr,
    pub db: i64,
    pub username: Option<String>,
    pub password: Option<String>,
    pub tls_params: Option<RedisTlsParams>,
}

#[derive(Clone, Debug, Default)]
pub struct RedisTlsParams {
    pub local_cert: Option<PathBuf>,
    pub local_pk: Option<PathBuf>,
    pub ca_file: Option<PathBuf>,
    pub accept_invalid_hostname: bool,
    pub insecure: bool,
}

impl RedisConfig {
    pub fn addr(&self) -> impl Iterator<Item = &RedisConnectionAddr> {
        match self {
            RedisConfig::Single(conn) => Either::Left(once(&conn.addr)),
            RedisConfig::Cluster(cluster) => Either::Right(cluster.addr.iter()),
        }
    }

    pub fn db(&self) -> i64 {
        match self {
            RedisConfig::Single(conn) => conn.db,
            RedisConfig::Cluster(cluster) => cluster.db,
        }
    }

    pub fn username(&self) -> Option<&str> {
        match self {
            RedisConfig::Single(conn) => conn.username.as_deref(),
            RedisConfig::Cluster(cluster) => cluster.username.as_deref(),
        }
    }

    pub fn passwd(&self) -> Option<&str> {
        match self {
            RedisConfig::Single(conn) => conn.password.as_deref(),
            RedisConfig::Cluster(cluster) => cluster.password.as_deref(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            RedisConfig::Single(_) => false,
            RedisConfig::Cluster(cluster) => cluster.addr.is_empty(),
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum DbConnect {
    Tcp { host: String, port: u16 },
    Socket(PathBuf),
}

impl Database {
    pub fn url(&self) -> String {
        match self {
            Database::Sqlite { database } => {
                format!("sqlite://{}", database.display())
            }
            Database::MySql {
                database,
                username,
                password,
                connect,
                ssl_options,
            } => {
                let mut params = Serializer::new(String::new());
                match ssl_options {
                    SslOptions::Default => {}
                    SslOptions::Disabled => {
                        params.append_pair("ssl-mode", "disabled");
                    }
                    SslOptions::Enabled { ca, verify, .. } => {
                        params.append_pair(
                            "ssl-mode",
                            if *verify {
                                "verify_identity"
                            } else {
                                "verify_ca"
                            },
                        );
                        params.append_pair("ssl-ca", ca.as_str());
                    }
                }
                let (host, port) = match connect {
                    DbConnect::Socket(socket) => {
                        params.append_pair("socket", &socket.to_string_lossy());
                        ("localhost", 3306) // ignored when socket is set
                    }
                    DbConnect::Tcp { host, port } => (host.as_str(), *port),
                };
                let params = params.finish().replace("%2F", "/");
                let params_start = if params.is_empty() { "" } else { "?" };

                if port == 3306 {
                    format!(
                        "mysql://{}:{}@{}/{}{}{}",
                        urlencoding::encode(username),
                        urlencoding::encode(password),
                        host,
                        database,
                        params_start,
                        params
                    )
                } else {
                    format!(
                        "mysql://{}:{}@{}:{}/{}{}{}",
                        urlencoding::encode(username),
                        urlencoding::encode(password),
                        host,
                        port,
                        database,
                        params_start,
                        params
                    )
                }
            }
            Database::Postgres {
                database,
                username,
                password,
                connect,
                ssl_options,
            } => {
                let mut params = Serializer::new(String::new());
                match ssl_options {
                    SslOptions::Default => {}
                    SslOptions::Disabled => {
                        params.append_pair("sslmode", "disable");
                    }
                    SslOptions::Enabled { ca, verify, .. } => {
                        params.append_pair(
                            "ssl-mode",
                            if *verify { "verify-full" } else { "verify-ca" },
                        );
                        params.append_pair("sslrootcert", ca.as_str());
                    }
                }
                let (host, port) = match connect {
                    DbConnect::Socket(socket) => {
                        params.append_pair("host", &socket.to_string_lossy());
                        ("localhost", 5432) // ignored when socket is set
                    }
                    DbConnect::Tcp { host, port } => (host.as_str(), *port),
                };
                let params = params.finish().replace("%2F", "/");
                let params_start = if params.is_empty() { "" } else { "?" };

                if port == 5432 {
                    format!(
                        "postgresql://{}:{}@{}/{}{}{}",
                        urlencoding::encode(username),
                        urlencoding::encode(password),
                        host,
                        database,
                        params_start,
                        params
                    )
                } else {
                    format!(
                        "postgresql://{}:{}@{}:{}/{}{}{}",
                        urlencoding::encode(username),
                        urlencoding::encode(password),
                        host,
                        port,
                        database,
                        params_start,
                        params
                    )
                }
            }
        }
    }
}

fn split_host(host: &str) -> (&str, Option<u16>, Option<&str>) {
    if host.starts_with('/') {
        return ("localhost", None, Some(host));
    }
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
