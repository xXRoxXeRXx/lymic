use sqlx::SqlitePool;

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct WatchedFolder {
    pub id: i64,
    pub path: String,
    pub recursive: bool,
    pub target_album_id: Option<String>,
}

pub async fn add_folder(pool: &SqlitePool, path: &str) -> Result<i64, sqlx::Error> {
    let result = sqlx::query("INSERT INTO watched_folders (path) VALUES (?)")
        .bind(path)
        .execute(pool)
        .await?;
    Ok(result.last_insert_rowid())
}

pub async fn restore_folder(
    pool: &SqlitePool,
    path: &str,
    recursive: bool,
    target_album_id: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT OR IGNORE INTO watched_folders (path, recursive, target_album_id) VALUES (?, ?, ?)",
    )
    .bind(path)
    .bind(recursive)
    .bind(target_album_id)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn get_folders(pool: &SqlitePool) -> Result<Vec<WatchedFolder>, sqlx::Error> {
    sqlx::query_as::<_, WatchedFolder>(
        "SELECT id, path, recursive, target_album_id FROM watched_folders",
    )
    .fetch_all(pool)
    .await
}

pub async fn remove_folder(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM watched_folders WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
