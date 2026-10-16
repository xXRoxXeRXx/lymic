use crate::app::events::log_to_ui_key;
use crate::audit::{audit_event, audit_safe_error};
use crate::{auth, immich};
use serde_json::json;
use tauri::Manager;

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
    let result = async {
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
        )
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
    // A logged-out projection cannot hydrate and must not remain visible as a
    // stale copy of the previous user's library.
    if let Some(root) = app.try_state::<crate::virtual_library::sync_root::SyncRootLifecycle>() {
        root.disable()?;
    }
    let result = auth::delete_credentials();
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

#[cfg(test)]
mod tests {
    use super::normalize_cloudflare_access_credentials;

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
pub(crate) async fn get_current_user_name() -> Result<String, String> {
    let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
    let client = immich::ImmichClient::new(
        credentials.server_url,
        credentials.api_key,
        credentials.allow_local_http,
        credentials.cloudflare_client_id,
        credentials.cloudflare_client_secret,
    )?;
    client.current_user().await
}
