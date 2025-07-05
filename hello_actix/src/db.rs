// Pattern extracted from the official SQLite example
// https://github.com/actix/examples/blob/master/databases/sqlite/src/db.rs
use chrono::{DateTime, Utc};

use actix_web::{error, web, Error};

pub type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
//
// pub type Connection = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

pub const DB_FILE: &str = "api-db.sqlite";

use rusqlite::{
    types::{FromSql, FromSqlError, ToSqlOutput},
    ToSql,
};

/// Deliberately not marked async, because it is not intended to be used while
/// the web API itself is live.
pub fn setup(pool: Pool) {
    let conn = pool.get().expect("unable to connect to the database");

    conn.execute(
        "
    CREATE TABLE IF NOT EXISTS usage (
        id INTEGER PRIMARY KEY,
        api_key TEXT,
        endpoint TEXT,
        called_at TEXT
    );",
        (),
    )
    .expect("unable to create `usage` table");

    conn.execute(
        "
    CREATE TABLE IF NOT EXISTS api_keys (
        id INTEGER PRIMARY KEY,
        salt TEXT,
        api_key TEXT,
        created_at TEXT NOT NULL,
        revoked_at TEXT
    );",
        (),
    )
    .expect("unable to create `api_keys` table");

    conn.execute(
        "
    CREATE INDEX IF NOT EXISTS api_keys_api_key_idx 
    ON api_keys (api_key);
  ",
        (),
    )
    .expect("unable to create `api_keys_api_key_idx` index");
}

#[derive(Debug)]
pub enum ApiEndpoint {
    ToCelsius,
    ToFahrenheit,
}

impl ApiEndpoint {
    fn as_str(&self) -> &str {
        match self {
            ApiEndpoint::ToCelsius => "to-celsius",
            ApiEndpoint::ToFahrenheit => "to-fahrenheit",
        }
    }
}

#[derive(Debug)]
pub struct UnknownApiEndpoint(String);

impl std::fmt::Display for UnknownApiEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown API endpoint ({})", self.0)
    }
}

impl std::error::Error for UnknownApiEndpoint {}

impl std::str::FromStr for ApiEndpoint {
    type Err = UnknownApiEndpoint;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "to-celsius" => Ok(ApiEndpoint::ToCelsius),
            "to-fahrenheit" => Ok(ApiEndpoint::ToFahrenheit),
            _ => Err(UnknownApiEndpoint(s.to_string())),
        }
    }
}

impl ToSql for ApiEndpoint {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for ApiEndpoint {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let endpoint: Self = value
            .as_str()?
            .parse()
            .map_err(|err| FromSqlError::Other(Box::new(err)))?;

        Ok(endpoint)
    }
}

pub enum Query {
    // CheckApiKey(String),
    RecordApiUsage {
        api_key: String,
        endpoint: ApiEndpoint,
        called_at: DateTime<Utc>,
    },
    RevokeApiKey(String),
    StoreApiKey {
        salt: String,
        api_key: String,
    },
}

impl Query {
    pub async fn execute(self, database: web::Data<Pool>) -> Result<Option<bool>, Error> {
        let conn = web::block(move || database.get())
            .await?
            .map_err(error::ErrorInternalServerError)?;

        match self {
            // Query::CheckApiKey(key) => {
            //     let sql = "
            //     SELECT 1
            //     FROM api_keys
            //     WHERE api_key = ?1 AND revoked_at IS NULL
            //     ";

            //     let mut stmt = conn
            //         .prepare_cached(sql)
            //         .map_err(error::ErrorInternalServerError)?;

            //     let result: Option<i32> = stmt
            //         .query_row((key,), |row| row.get(0))
            //         .optional()
            //         .map_err(error::ErrorInternalServerError)?;

            //     Ok(Some(result.is_some()))
            // }
            Query::RecordApiUsage {
                api_key,
                endpoint,
                called_at,
            } => {
                let sql = "
                INSERT INTO usage (api_key, endpoint, called_at) 
                VALUES (?1, ?2, ?3);
                ";

                let mut stmt = conn
                    .prepare_cached(sql)
                    .map_err(error::ErrorInternalServerError)?;

                let _n_rows = stmt
                    .execute((api_key, endpoint, called_at))
                    .map_err(error::ErrorInternalServerError)?;

                Ok(None)
            }
            Query::StoreApiKey { api_key, salt } => {
                let sql = "
                INSERT INTO api_keys (api_key, salt, created_at)
                VALUES (?1, ?2, ?3);
                ";

                let now = Utc::now();

                let mut stmt = conn
                    .prepare_cached(sql)
                    .map_err(error::ErrorInternalServerError)?;

                let _n_rows = stmt
                    .execute((api_key, salt, now))
                    .map_err(error::ErrorInternalServerError)?;

                Ok(None)
            }
            Query::RevokeApiKey(key) => {
                let sql = "
                UPDATE api_keys
                SET revoked_at = ?1 
                WHERE api_key = ?2;
                ";

                let now = Utc::now();

                let mut stmt = conn
                    .prepare_cached(sql)
                    .map_err(error::ErrorInternalServerError)?;

                let _n_rows = stmt
                    .execute((now, key))
                    .map_err(error::ErrorInternalServerError)?;

                Ok(None)
            }
        }
    }
}
