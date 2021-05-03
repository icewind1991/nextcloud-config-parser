mod nc;

use redis::{ConnectionAddr, ConnectionInfo};
use sqlx::any::AnyConnectOptions;
use std::iter::once;
use std::path::PathBuf;
use thiserror::Error;

pub use nc::parse;

#[derive(Debug)]
pub struct Config {
    pub database: AnyConnectOptions,
    pub database_prefix: String,
    pub redis: RedisConfig,
    pub nextcloud_url: String,
}

#[derive(Debug)]
pub enum RedisConfig {
    Single(ConnectionInfo),
    Cluster(Vec<ConnectionInfo>),
}

impl RedisConfig {
    pub fn addr(&self) -> impl Iterator<Item = &ConnectionAddr> {
        let boxed: Box<dyn Iterator<Item = &ConnectionAddr>> = match self {
            RedisConfig::Single(conn) => Box::new(once(conn.addr.as_ref())),
            RedisConfig::Cluster(conns) => Box::new(conns.iter().map(|conn| conn.addr.as_ref())),
        };
        boxed
    }

    pub fn db(&self) -> i64 {
        match self {
            RedisConfig::Single(conn) => conn.db,
            RedisConfig::Cluster(conns) => conns.first().map(|conn| conn.db).unwrap_or_default(),
        }
    }

    pub fn username(&self) -> Option<&str> {
        match self {
            RedisConfig::Single(conn) => conn.username.as_deref(),
            RedisConfig::Cluster(conns) => conns
                .first()
                .map(|conn| conn.username.as_deref())
                .unwrap_or_default(),
        }
    }

    pub fn passwd(&self) -> Option<&str> {
        match self {
            RedisConfig::Single(conn) => conn.passwd.as_deref(),
            RedisConfig::Cluster(conns) => conns
                .first()
                .map(|conn| conn.passwd.as_deref())
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

#[derive(Debug, Error)]
pub enum Error {
    #[error("Error while parsing php literal: {0:#}")]
    Php(#[from] php_literal_parser::ParseError),
    #[error("Provided config file doesn't seem to be a nextcloud config file: {0:#}")]
    NotAConfig(#[from] NotAConfigError),
    #[error("Failed to read config file")]
    ReadFailed(std::io::Error, PathBuf),
    #[error("unsupported database type {0}")]
    UnsupportedDb(String),
    #[error("no database configuration")]
    NoDb,
    #[error("Invalid redis configuration")]
    Redis,
    #[error("`overwrite.cli.url` not set`")]
    NoUrl,
}

#[derive(Debug, Error)]
pub enum NotAConfigError {
    #[error("$CONFIG not found in file")]
    NoConfig(PathBuf),
    #[error("$CONFIG is not an array")]
    NotAnArray(PathBuf),
}
