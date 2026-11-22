//! Which directories of the Virtual Library have been populated (#280): listed once through the
//! projection and handed to the Windows platform. Reconciliation only diffs these. Paths are
//! relative to the Virtual Library root with `/` separators; the root is the empty string.

use sqlx::SqlitePool;

/// Records that `path` was populated. Recording it again changes nothing.
pub async fn mark_virtual_library_directory_populated(
    pool: &SqlitePool,
    path: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO virtual_library_populated_directories (path) VALUES (?)")
        .bind(path)
        .execute(pool)
        .await?;
    Ok(())
}

/// Every populated directory, in no particular order.
pub async fn list_virtual_library_populated_directories(
    pool: &SqlitePool,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT path FROM virtual_library_populated_directories")
        .fetch_all(pool)
        .await
}

/// Forgets `path` and every directory below it. The root (`""`) forgets everything.
pub async fn forget_virtual_library_populated_tree(
    pool: &SqlitePool,
    path: &str,
) -> Result<(), sqlx::Error> {
    if path.is_empty() {
        return clear_virtual_library_populated_directories(pool).await;
    }
    sqlx::query(
        "DELETE FROM virtual_library_populated_directories \
         WHERE path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '/'",
    )
    .bind(path)
    .execute(pool)
    .await?;
    Ok(())
}

/// Forgets every populated directory, for a Virtual Library folder that is not the same one any more.
pub async fn clear_virtual_library_populated_directories(
    pool: &SqlitePool,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM virtual_library_populated_directories")
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    async fn all(pool: &SqlitePool) -> Vec<String> {
        let mut paths = list_virtual_library_populated_directories(pool)
            .await
            .unwrap();
        paths.sort();
        paths
    }

    #[tokio::test]
    async fn a_populated_directory_is_remembered_once() {
        let pool = test_pool().await;

        mark_virtual_library_directory_populated(&pool, "Albums/Trip")
            .await
            .unwrap();
        mark_virtual_library_directory_populated(&pool, "Albums/Trip")
            .await
            .unwrap();
        mark_virtual_library_directory_populated(&pool, "")
            .await
            .unwrap();

        assert_eq!(all(&pool).await, vec!["", "Albums/Trip"]);
    }

    #[tokio::test]
    async fn forgetting_a_directory_forgets_what_is_below_it_but_not_similar_names() {
        let pool = test_pool().await;
        for path in [
            "",
            "Albums",
            "Albums/Trip",
            "Albums/Trip/x",
            "Albums/Trip 2",
        ] {
            mark_virtual_library_directory_populated(&pool, path)
                .await
                .unwrap();
        }

        forget_virtual_library_populated_tree(&pool, "Albums/Trip")
            .await
            .unwrap();

        assert_eq!(all(&pool).await, vec!["", "Albums", "Albums/Trip 2"]);
    }

    #[tokio::test]
    async fn names_with_like_wildcards_are_matched_literally() {
        let pool = test_pool().await;
        for path in ["Albums/100%", "Albums/1000"] {
            mark_virtual_library_directory_populated(&pool, path)
                .await
                .unwrap();
        }

        forget_virtual_library_populated_tree(&pool, "Albums/100%")
            .await
            .unwrap();

        assert_eq!(all(&pool).await, vec!["Albums/1000"]);
    }

    #[tokio::test]
    async fn forgetting_the_root_or_clearing_forgets_everything() {
        let pool = test_pool().await;
        for path in ["", "Albums"] {
            mark_virtual_library_directory_populated(&pool, path)
                .await
                .unwrap();
        }
        forget_virtual_library_populated_tree(&pool, "")
            .await
            .unwrap();
        assert!(all(&pool).await.is_empty());

        mark_virtual_library_directory_populated(&pool, "Albums")
            .await
            .unwrap();
        clear_virtual_library_populated_directories(&pool)
            .await
            .unwrap();
        assert!(all(&pool).await.is_empty());
    }
}
