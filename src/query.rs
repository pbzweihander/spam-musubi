use deadpool_postgres::tokio_postgres::error::Error as PgError;
use deadpool_postgres::{tokio_postgres::NoTls, Config, CreatePoolError, Pool, Runtime};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum QueryError {
	#[error("Database error: {0}")]
	DbError(#[from] PgError),
	#[error("Pool error: {0}")]
	PoolError(#[from] deadpool_postgres::PoolError),
}

#[derive(Clone)]
pub struct Query {
	pool: Pool,
}

pub struct User {
	pub followers: i32,
	pub following: i32,
	pub notes: i32,
}

pub struct InstanceStats {
	pub followers: i32,
	pub following: i32,
	pub notes: i32,
}

impl Query {
	pub fn init(
		host: &str, user: &str, password: &str, db_name: &str,
	) -> Result<Self, CreatePoolError> {
		let mut cfg = Config::new();
		cfg.host = Some(host.to_owned());
		cfg.user = Some(user.to_owned());
		cfg.password = Some(password.to_owned());
		cfg.dbname = Some(db_name.to_owned());

		let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)?;

		Ok(Query { pool })
	}

	pub async fn get_user(&self, uri: &str) -> Result<Option<User>, QueryError> {
		let client = self.pool.get().await?;
		let row = client
			.query(
				"SELECT followersCount, followingCount, notesCount FROM user WHERE uri = $1",
				&[&uri],
			)
			.await?;

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
		let row = client
			.query(
				"SELECT followersCount, followingCount, notesCount FROM instance WHERE host = $1",
				&[&host],
			)
			.await?;

		Ok(row.first().map(|row| InstanceStats {
			followers: row.get(0),
			following: row.get(1),
			notes: row.get(2),
		}))
	}
}
