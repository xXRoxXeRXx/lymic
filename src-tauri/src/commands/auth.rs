use crate::app::events::log_to_ui;
use crate::audit::{audit_event, audit_safe_error};
use crate::{auth, immich};
use serde_json::json;

// (security): api_key is intentionally kept out of all log and error messages below.
#[tauri::command]
pub(crate) async fn login(
    app: tauri::AppHandle,
    server_url: String,
    api_key: String,
) -> Result<(), String> {
    log_to_ui(&app, "INFO", "Attempting to connect to server");
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
        let temp_client = immich::ImmichClient::new(server_url.clone(), api_key.clone())?;
        temp_client.validate_connection().await?;
        auth::store_credentials(&server_url, &api_key)
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
            json!({ "failure_reason": result.as_ref().err().map(|error| audit_safe_error(error, Some(&server_url), Some(&api_key))) }),
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
            json!({ "failure_reason": result.as_ref().err().map(|error| audit_safe_error(error, None, None)) }),
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
pub(crate) async fn get_current_user_name() -> Result<String, String> {
    let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
    let client = immich::ImmichClient::new(credentials.server_url, credentials.api_key)?;
    client.current_user().await
}
