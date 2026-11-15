use super::seams::{ExclusionPatterns, FolderEffects, FolderPersistence, FolderSyncWork};

pub(crate) async fn update_folder_exclusion_patterns<P, S, E>(
    persistence: &P,
    sync: &S,
    effects: &E,
    id: i64,
    exclusion_patterns: Vec<String>,
) -> Result<(), String>
where
    P: FolderPersistence,
    S: FolderSyncWork,
    E: FolderEffects,
{
    let exclusion_patterns = ExclusionPatterns::parse(exclusion_patterns)?;
    if !persistence
        .update_exclusion_patterns(id, &exclusion_patterns)
        .await?
    {
        return Err("Watched folder was not found.".to_string());
    }
    let folder = persistence
        .list_folders()
        .await?
        .into_iter()
        .find(|folder| folder.id == id)
        .ok_or_else(|| "Watched folder was not found.".to_string())?;
    let scan_result = sync.scan_folder(folder).await?;
    effects
        .admit_rescan(
            scan_result,
            crate::sync::SyncSource::ExclusionPatternsChanged,
        )
        .await;
    Ok(())
}
