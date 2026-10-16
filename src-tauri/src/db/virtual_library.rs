use sqlx::{Sqlite, SqlitePool, Transaction};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VirtualLibraryBinding {
    pub(crate) asset_id: String,
    pub(crate) projection_path: String,
    pub(crate) version: String,
}

/// Replaces a completed reconciliation snapshot atomically.  An interrupted run
/// never advances the watermark, so the next activation safely replays it.
pub(crate) async fn replace_virtual_library_snapshot(
    pool: &SqlitePool,
    bindings: &[VirtualLibraryBinding],
    watermark: &str,
) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM virtual_library_bindings")
        .execute(&mut *transaction)
        .await?;
    insert_bindings(&mut transaction, bindings).await?;
    sqlx::query(
        "INSERT INTO virtual_library_reconciliation (id, watermark) VALUES (1, ?) \
         ON CONFLICT(id) DO UPDATE SET watermark = excluded.watermark",
    )
    .bind(watermark)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await
}

async fn insert_bindings(
    transaction: &mut Transaction<'_, Sqlite>,
    bindings: &[VirtualLibraryBinding],
) -> Result<(), sqlx::Error> {
    for binding in bindings {
        sqlx::query(
            "INSERT INTO virtual_library_bindings (asset_id, projection_path, version) VALUES (?, ?, ?)",
        )
        .bind(&binding.asset_id)
        .bind(&binding.projection_path)
        .bind(&binding.version)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) async fn virtual_library_bindings(
    pool: &SqlitePool,
) -> Result<Vec<VirtualLibraryBinding>, sqlx::Error> {
    sqlx::query_as::<_, (String, String, String)>(
        "SELECT asset_id, projection_path, version FROM virtual_library_bindings ORDER BY projection_path",
    )
    .fetch_all(pool)
    .await
    .map(|rows| rows.into_iter().map(|(asset_id, projection_path, version)| VirtualLibraryBinding { asset_id, projection_path, version }).collect())
}
