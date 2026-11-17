use super::seams::{ExclusionPatterns, FolderEffects, FolderPersistence};

pub(crate) async fn update_folder_exclusion_patterns<P, E>(
    persistence: &P,
    effects: &E,
    id: i64,
    exclusion_patterns: Vec<String>,
) -> Result<(), String>
where
    P: FolderPersistence,
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
    effects
        .scan_and_admit(folder, crate::sync::SyncSource::ExclusionPatternsChanged)
        .await
}
