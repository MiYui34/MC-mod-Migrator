use std::path::Path;

use rusqlite::params;

use crate::db::AppDatabase;
use crate::models::MigrationRecord;

pub fn ensure_history_table(db: &AppDatabase) -> anyhow::Result<()> {
    db.with_conn(|conn| {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS migration_history (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                source_name TEXT NOT NULL,
                target_name TEXT NOT NULL,
                source_mc TEXT NOT NULL,
                target_mc TEXT NOT NULL,
                category TEXT NOT NULL,
                success INTEGER NOT NULL,
                failed INTEGER NOT NULL,
                skipped INTEGER NOT NULL,
                backup_id TEXT,
                manifest_path TEXT,
                report_path TEXT
            );
            ",
        )?;
        Ok(())
    })
}

pub fn record_migration(data_dir: &Path, record: MigrationRecord) -> anyhow::Result<()> {
    let db = AppDatabase::new(data_dir.to_path_buf())?;
    ensure_history_table(&db)?;
    db.with_conn(|conn| {
        conn.execute(
            "INSERT OR REPLACE INTO migration_history
             (id, timestamp, source_name, target_name, source_mc, target_mc, category,
              success, failed, skipped, backup_id, manifest_path, report_path)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                record.id,
                record.timestamp,
                record.source_name,
                record.target_name,
                record.source_mc,
                record.target_mc,
                record.category,
                record.success,
                record.failed,
                record.skipped,
                record.backup_id,
                record.manifest_path,
                record.report_path,
            ],
        )?;
        Ok(())
    })
}

pub fn list_migration_history(data_dir: &Path) -> anyhow::Result<Vec<MigrationRecord>> {
    let db = AppDatabase::new(data_dir.to_path_buf())?;
    ensure_history_table(&db)?;
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, source_name, target_name, source_mc, target_mc, category,
                    success, failed, skipped, backup_id, manifest_path, report_path
             FROM migration_history ORDER BY timestamp DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MigrationRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                source_name: row.get(2)?,
                target_name: row.get(3)?,
                source_mc: row.get(4)?,
                target_mc: row.get(5)?,
                category: row.get(6)?,
                success: row.get::<_, i64>(7)? as u32,
                failed: row.get::<_, i64>(8)? as u32,
                skipped: row.get::<_, i64>(9)? as u32,
                backup_id: row.get(10)?,
                manifest_path: row.get(11)?,
                report_path: row.get(12)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    })
}

pub fn delete_migration_record(data_dir: &Path, id: &str) -> anyhow::Result<()> {
    let db = AppDatabase::new(data_dir.to_path_buf())?;
    ensure_history_table(&db)?;
    db.with_conn(|conn| {
        conn.execute("DELETE FROM migration_history WHERE id = ?1", params![id])?;
        Ok(())
    })
}
