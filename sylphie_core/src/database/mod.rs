use crate::errors::*;
use r2d2::{Pool, ManageConnection, PooledConnection};
use rusqlite::{Connection, OpenFlags, Result as RusqliteResult, Error as RusqliteError};
use static_events::*;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::time;
use std::sync::Arc;

// TODO: Write a wrapper around `rusqlite` to deal with its frequent major version changes.

struct ConnectionManager {
    db_file: PathBuf,
    transient_db_file: PathBuf,
}
impl ConnectionManager {
    fn new(path: PathBuf, transient_path: PathBuf) -> Result<ConnectionManager> {
        Ok(ConnectionManager {
            db_file: path,
            transient_db_file: transient_path,
        })
    }
}
impl ManageConnection for ConnectionManager {
    type Connection = Connection;
    type Error = RusqliteError;

    fn connect(&self) -> RusqliteResult<Connection> {
        let conn = Connection::open_with_flags(&self.db_file,
            OpenFlags::SQLITE_OPEN_READ_WRITE |
            OpenFlags::SQLITE_OPEN_CREATE)?;
        conn.set_prepared_statement_cache_capacity(64);
        conn.execute_batch(include_str!("setup_connection.sql"))?;
        conn.execute(
            r#"ATTACH DATABASE ? AS transient;"#,
            &[self.transient_db_file.to_str().expect("Could not convert path to str.")],
        )?;
        Ok(conn)
    }
    fn is_valid(&self, conn: &mut Connection) -> RusqliteResult<()> {
        conn.prepare_cached("SELECT 1")?.query_row(&[0i32; 0], |_| Ok(()))?;
        Ok(())
    }
    fn has_broken(&self, _: &mut Connection) -> bool {
        false
    }
}

// TODO: Track active connection count for shutdown?
pub struct DatabaseConnection {
    conn: PooledConnection<ConnectionManager>,
}
impl DatabaseConnection {
    fn new(conn: PooledConnection<ConnectionManager>) -> DatabaseConnection {
        DatabaseConnection { conn, }
    }

    pub fn checkpoint(&self) -> Result<()> {
        self.conn.execute_batch("PRAGMA wal_checkpoint(RESTART)")?;
        Ok(())
    }
}
impl Deref for DatabaseConnection {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}
impl DerefMut for DatabaseConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.conn
    }
}

#[derive(Clone)]
pub struct Database {
    pool: Arc<Pool<ConnectionManager>>,
}
impl Database {
    pub(crate) fn new(path: PathBuf, transient_path: PathBuf) -> Result<Database> {
        let pool = Arc::new(Pool::builder()
            .max_size(15)
            .idle_timeout(Some(time::Duration::from_secs(60 * 5)))
            .build(ConnectionManager::new(path, transient_path)?)?);
        Ok(Database { pool })
    }

    pub fn connect(&self) -> Result<DatabaseConnection> {
        Ok(DatabaseConnection::new(self.pool.get()?))
    }
}
