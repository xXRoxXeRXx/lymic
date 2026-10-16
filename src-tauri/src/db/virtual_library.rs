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

pub(crate) async fn virtual_library_reconciliation_cursor(
    pool: &SqlitePool,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT watermark FROM virtual_library_reconciliation WHERE id = 1")
        .fetch_optional(pool)
        .await
}

/// Removes durable bindings only after their corresponding placeholder was
/// removed locally. Callers retain bindings that could not be removed (for
/// example, because Explorer still has the file open) so the next run retries.
pub(crate) async fn remove_virtual_library_bindings(
    pool: &SqlitePool,
    bindings: &[VirtualLibraryBinding],
) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    for binding in bindings {
        sqlx::query(
            "DELETE FROM virtual_library_bindings WHERE asset_id = ? AND projection_path = ?",
        )
        .bind(&binding.asset_id)
        .bind(&binding.projection_path)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await
}

/// Publishes an incremental projection only after every fetched page has been
/// projected successfully. The cursor is the commit point: a failed run will
/// be replayed from the previous value.
pub(crate) async fn apply_virtual_library_delta(
    pool: &SqlitePool,
    bindings: &[VirtualLibraryBinding],
    cursor: &str,
) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    for binding in bindings {
        sqlx::query(
            "INSERT INTO virtual_library_bindings (asset_id, projection_path, version) VALUES (?, ?, ?) \
             ON CONFLICT(asset_id, projection_path) DO UPDATE SET version = excluded.version",
        )
        .bind(&binding.asset_id)
        .bind(&binding.projection_path)
        .bind(&binding.version)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query(
        "INSERT INTO virtual_library_reconciliation (id, watermark) VALUES (1, ?) \
         ON CONFLICT(id) DO UPDATE SET watermark = excluded.watermark",
    )
    .bind(cursor)
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
