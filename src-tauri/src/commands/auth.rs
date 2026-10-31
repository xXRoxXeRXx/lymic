use crate::app::events::log_to_ui_key;
use crate::audit::{audit_event, audit_safe_error};
use crate::commands::profile_cache::{self, CurrentUser};
use crate::{auth, immich};
use serde_json::json;
use std::path::PathBuf;
use tauri::Manager;

const PROFILE_CACHE_DIR: &str = "profile_cache";

pub(crate) fn profile_cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("Failed to get app cache dir: {e}"))?;
    Ok(cache_root.join(PROFILE_CACHE_DIR))
}

fn normalize_cloudflare_access_credentials(
    client_id: Option<String>,
    client_secret: Option<String>,
) -> (Option<String>, Option<String>) {
    (
        client_id.and_then(|value| (!value.trim().is_empty()).then(|| value.trim().to_string())),
        client_secret
            .and_then(|value| (!value.trim().is_empty()).then(|| value.trim().to_string())),
    )
}

fn safe_profile_error(error: &str, credentials: &auth::AuthConfig) -> String {
    audit_safe_error(
        error,
        &[
            Some(&credentials.server_url),
            Some(&credentials.api_key),
            credentials.cloudflare_client_id.as_deref(),
            credentials.cloudflare_client_secret.as_deref(),
        ],
    )
}

// (security): credential values are intentionally kept out of all log and error messages below.
#[tauri::command]
pub(crate) async fn login(
    app: tauri::AppHandle,
    server_url: String,
    api_key: String,
    allow_local_http: bool,
    cloudflare_client_id: Option<String>,
    cloudflare_client_secret: Option<String>,
) -> Result<(), String> {
    let (cloudflare_client_id, cloudflare_client_secret) =
        normalize_cloudflare_access_credentials(cloudflare_client_id, cloudflare_client_secret);
    log_to_ui_key(
        &app,
        "INFO",
        "Attempting to connect to server",
        "log_connecting_to_server",
    );
    let operation_id = crate::audit::AuditEvent::operation_id();
    let actor_id = Some(crate::audit::actor_id(&server_url, &api_key));
    audit_event(
        &app,
        crate::audit::AuditEvent::new(
            &operation_id,
            "auth.login",
            crate::audit::Outcome::Started,
            crate::audit::Severity::Info,
            actor_id.clone(),
            "command",
            "Login started",
            json!({}),
        ),
    );

    // Build a temporary ImmichClient purely to reuse its URL normalisation and
    // pooled reqwest Client. The client is discarded after the connection test.
    let result: Result<(), String> = async {
        let temp_client = immich::ImmichClient::new(
            server_url.clone(),
            api_key.clone(),
            allow_local_http,
            cloudflare_client_id.clone(),
            cloudflare_client_secret.clone(),
        )?;
        temp_client.validate_connection().await?;
        auth::store_credentials(
            &server_url,
            &api_key,
            temp_client.uses_http(),
            cloudflare_client_id.clone(),
            cloudflare_client_secret.clone(),
        )?;
        if let Ok(cache_dir) = profile_cache_dir(&app) {
            profile_cache::clear_profile_cache(&cache_dir);
        }
        Ok(())
    }
    .await;
    audit_event(
        &app,
        crate::audit::AuditEvent::new(
            &operation_id,
            "auth.login",
            if result.is_ok() {
                crate::audit::Outcome::Success
            } else {
                crate::audit::Outcome::Failure
            },
            if result.is_ok() {
                crate::audit::Severity::Info
            } else {
                crate::audit::Severity::Error
            },
            actor_id,
            "command",
            if result.is_ok() {
                "Login completed"
            } else {
                "Login failed"
            },
            json!({ "failure_reason": result.as_ref().err().map(|error| audit_safe_error(error, &[Some(&server_url), Some(&api_key), cloudflare_client_id.as_deref(), cloudflare_client_secret.as_deref()])) }),
        ),
    );
    result
}

#[tauri::command]
pub(crate) async fn logout(app: tauri::AppHandle) -> Result<(), String> {
    let operation_id = crate::audit::AuditEvent::operation_id();
    let actor_id = auth::get_credentials()
        .ok()
        .flatten()
        .map(|credentials| crate::audit::actor_id(&credentials.server_url, &credentials.api_key));
    let result = auth::delete_credentials();
    if result.is_ok() {
        if let Ok(cache_dir) = profile_cache_dir(&app) {
            profile_cache::clear_profile_cache(&cache_dir);
        }
    }
    audit_event(
        &app,
        crate::audit::AuditEvent::new(
            operation_id,
            "auth.logout",
            if result.is_ok() {
                crate::audit::Outcome::Success
            } else {
                crate::audit::Outcome::Failure
            },
            if result.is_ok() {
                crate::audit::Severity::Info
            } else {
                crate::audit::Severity::Error
            },
            actor_id,
            "command",
            if result.is_ok() {
                "Logout completed"
            } else {
                "Logout failed"
            },
            json!({ "failure_reason": result.as_ref().err().map(|error| audit_safe_error(error, &[])) }),
        ),
    );
    result
}

#[tauri::command]
pub(crate) async fn get_auth_status() -> Result<bool, String> {
    Ok(auth::get_credentials()?.is_some())
}

#[tauri::command]
pub(crate) async fn get_server_url() -> Result<String, String> {
    if let Some(creds) = auth::get_credentials()? {
        Ok(creds.server_url)
    } else {
        Ok("".to_string())
    }
}

#[tauri::command]
pub(crate) async fn get_current_user(app: tauri::AppHandle) -> Result<CurrentUser, String> {
    let cache_dir = profile_cache_dir(&app)?;

    let credentials = match auth::get_credentials() {
        Ok(Some(creds)) => creds,
        Ok(None) => return Err("Not logged in".to_string()),
        Err(e) => return Err(e),
    };
    let account_id = crate::audit::actor_id(&credentials.server_url, &credentials.api_key);

    let client = match immich::ImmichClient::new(
        credentials.server_url.clone(),
        credentials.api_key.clone(),
        credentials.allow_local_http,
        credentials.cloudflare_client_id.clone(),
        credentials.cloudflare_client_secret.clone(),
    ) {
        Ok(c) => c,
        Err(err) => {
            eprintln!(
                "Failed to initialize Immich client: {}",
                safe_profile_error(&err, &credentials)
            );
            return Ok(profile_cache::resolve_profile(
                &cache_dir,
                &account_id,
                profile_cache::ProfileFetchOutcome::RefreshFailed,
            ));
        }
    };

    let outcome = match client.current_user_info().await {
        Ok(user_info) => {
            if user_info.has_profile_image {
                match client.get_profile_image(&user_info.id).await {
                    Ok((bytes, content_type)) => {
                        profile_cache::ProfileFetchOutcome::SuccessWithImage {
                            user_id: user_info.id,
                            display_name: user_info.display_name,
                            bytes,
                            content_type,
                        }
                    }
                    Err(err) => {
                        eprintln!(
                            "Failed to fetch profile image: {}",
                            safe_profile_error(&err, &credentials)
                        );
                        profile_cache::ProfileFetchOutcome::ImageFetchFailed {
                            user_id: user_info.id,
                            display_name: user_info.display_name,
                        }
                    }
                }
            } else {
                profile_cache::ProfileFetchOutcome::SuccessNoImage {
                    user_id: user_info.id,
                    display_name: user_info.display_name,
                }
            }
        }
        Err(err) => {
            eprintln!(
                "Failed to refresh current user: {}",
                safe_profile_error(&err, &credentials)
            );
            profile_cache::ProfileFetchOutcome::RefreshFailed
        }
    };

    // Guard against logout race: confirm credentials still match before resolving/persisting
    let current_creds = auth::get_credentials().ok().flatten();
    let is_still_same_account = current_creds
        .as_ref()
        .map(|c| crate::audit::actor_id(&c.server_url, &c.api_key) == account_id)
        .unwrap_or(false);

    if !is_still_same_account {
        return Err("Session changed during profile refresh".to_string());
    }

    Ok(profile_cache::resolve_profile(
        &cache_dir,
        &account_id,
        outcome,
    ))
}

#[cfg(test)]
mod tests {
    use super::{normalize_cloudflare_access_credentials, safe_profile_error};
    use crate::auth::AuthConfig;

    #[test]
    fn redacts_all_profile_credentials_from_diagnostic_errors() {
        let credentials = AuthConfig {
            server_url: "https://immich.example.test".to_string(),
            api_key: "api-key-secret".to_string(),
            allow_local_http: false,
            cloudflare_client_id: Some("cloudflare-client-id".to_string()),
            cloudflare_client_secret: Some("cloudflare-client-secret".to_string()),
        };
        let safe_error = safe_profile_error(
            "failed https://immich.example.test api-key-secret cloudflare-client-id cloudflare-client-secret",
            &credentials,
        );

        for secret in [
            "https://immich.example.test",
            "api-key-secret",
            "cloudflare-client-id",
            "cloudflare-client-secret",
        ] {
            assert!(!safe_error.contains(secret));
        }
    }

    #[test]
    fn normalizes_whitespace_only_cloudflare_access_credentials_to_disabled() {
        assert_eq!(
            normalize_cloudflare_access_credentials(
                Some(" \t ".to_string()),
                Some("\n".to_string())
            ),
            (None, None),
        );
    }
}
