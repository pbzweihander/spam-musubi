use clap::ValueEnum;
use deadpool_postgres::{
	tokio_postgres::{error::Error as PgError, NoTls},
	Config, CreatePoolError, Pool, PoolError, Runtime,
};
use thiserror::Error;

pub mod constants;

use constants::PreparedQueries;

#[derive(Error, Debug)]
pub enum QueryInitError {
	#[error("Config error: {0}")]
	Config(#[from] CreatePoolError),
	#[error("Connection failure: {0}")]
	ConnectionError(#[from] PoolError),
}

#[derive(Error, Debug)]
pub enum QueryError {
	#[error("Database error: {0}")]
	DbError(#[from] PgError),
	#[error("Pool error: {0}")]
	PoolError(#[from] PoolError),
}

#[derive(Clone)]
pub struct Query {
	pool: Pool,
	prepared_queries: PreparedQueries,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum QueryOpMode {
	Misskey,
	Mastodon,
}

#[derive(Debug)]
pub struct User {
	pub followers: i32,
	pub following: i32,
	pub notes: i32,
}

#[derive(Debug)]
pub struct InstanceStats {
	pub followers: i32,
	pub following: i32,
	pub notes: i32,
}

impl Query {
	pub async fn init(
		host: &str, port: u16, user: &str, password: &str, db_name: &str,
		query_op_mode: QueryOpMode,
	) -> Result<Self, QueryInitError> {
		let mut cfg = Config::new();
		cfg.host = Some(host.to_owned());
		cfg.port = Some(port);
		cfg.user = Some(user.to_owned());
		cfg.password = Some(password.to_owned());
		cfg.dbname = Some(db_name.to_owned());

		let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)?;
		// check if connection is successful
		let _ = pool.get().await?;

		Ok(Query { pool, prepared_queries: constants::get_prepared_queries(query_op_mode) })
	}

	pub async fn get_user(&self, uri: &str) -> Result<Option<User>, QueryError> {
		let client = self.pool.get().await?;
		let row = client.query(self.prepared_queries.get_user, &[&uri]).await?;

		Ok(row.first().map(|row| User {
			followers: row.get(0),
			following: row.get(1),
			notes: row.get(2),
		}))
	}

	pub async fn get_instance_stats(
		&self, host: &str,
	) -> Result<Option<InstanceStats>, QueryError> {
		let client = self.pool.get().await?;
		let row = client.query(self.prepared_queries.get_instance_stats, &[&host]).await?;

		Ok(row.first().map(|row| InstanceStats {
			followers: row.get(0),
			following: row.get(1),
			notes: row.get(2),
		}))
	}
}
