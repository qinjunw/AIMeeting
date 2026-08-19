use rusqlite::Connection;

use super::Result;

const INITIAL_SCHEMA: &str = include_str!("../../migrations/0001_initial.sql");

pub fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch(INITIAL_SCHEMA)?;
    Ok(())
}
