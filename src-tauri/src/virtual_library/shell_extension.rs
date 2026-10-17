//! Registration contract for Explorer's thumbnail provider.
//!
//! The installer owns the machine-wide association.  Lymic repeats the same
//! association in HKCU while a Virtual Library is enabled so a per-user install
//! never needs elevation and disabling the library removes its Explorer hook.

use std::path::Path;
#[cfg(windows)]
use tauri::Manager;

pub(crate) const PROVIDER_ID: &str = "{6B3AF5BC-6E32-40D8-B0A2-2C14B3FA481C}";
pub(crate) const THUMBNAIL_PROVIDER_CLSID: &str = "{D8D7474E-6E04-4534-8EF5-C4A90F01B395}";
pub(crate) const THUMBNAIL_PROVIDER_DLL: &str = "lymic-shell-ext.dll";

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegistryHive {
    CurrentUser,
}

impl RegistryHive {
    #[cfg(windows)]
    fn reg_name(self) -> &'static str {
        match self {
            Self::CurrentUser => "HKCU",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RegistryOperation {
    SetDefaultValue {
        key: String,
        value: String,
    },
    SetValue {
        key: String,
        name: String,
        value: String,
    },
    DeleteValue {
        key: String,
        name: String,
    },
    DeleteKey {
        key: String,
    },
}

fn class_key() -> String {
    format!("Software\\Classes\\CLSID\\{THUMBNAIL_PROVIDER_CLSID}")
}

fn sync_root_key() -> String {
    format!(
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\SyncRootManager\\{PROVIDER_ID}"
    )
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn registration_operations(dll_path: &Path) -> Vec<RegistryOperation> {
    let class_key = class_key();
    let inproc_key = format!("{class_key}\\InprocServer32");
    let dll_path_str = {
        let s = dll_path.to_string_lossy();
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
    };
    vec![
        RegistryOperation::SetDefaultValue {
            key: class_key,
            value: "Lymic Thumbnail Provider".to_string(),
        },
        RegistryOperation::SetDefaultValue {
            key: inproc_key.clone(),
            value: dll_path_str,
        },
        RegistryOperation::SetValue {
            key: inproc_key,
            name: "ThreadingModel".to_string(),
            value: "Apartment".to_string(),
        },
        RegistryOperation::SetValue {
            key: sync_root_key(),
            name: "ThumbnailProvider".to_string(),
            value: THUMBNAIL_PROVIDER_CLSID.to_string(),
        },
    ]
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn unregistration_operations() -> Vec<RegistryOperation> {
    vec![
        RegistryOperation::DeleteValue {
            key: sync_root_key(),
            name: "ThumbnailProvider".to_string(),
        },
        RegistryOperation::DeleteKey { key: class_key() },
    ]
}

#[cfg(windows)]
pub(crate) fn register_for_app(app: &tauri::AppHandle) -> Result<(), String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|error| format!("Could not locate application resources: {error}"))?;
    apply(
        RegistryHive::CurrentUser,
        registration_operations(&resource_dir.join(THUMBNAIL_PROVIDER_DLL)),
    )
}

#[cfg(not(windows))]
pub(crate) fn register_for_app(_: &tauri::AppHandle) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
pub(crate) fn unregister_for_current_user() -> Result<(), String> {
    apply(RegistryHive::CurrentUser, unregistration_operations())
}

#[cfg(not(windows))]
pub(crate) fn unregister_for_current_user() -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn apply(hive: RegistryHive, operations: Vec<RegistryOperation>) -> Result<(), String> {
    for operation in operations {
        let is_deletion = matches!(
            operation,
            RegistryOperation::DeleteValue { .. } | RegistryOperation::DeleteKey { .. }
        );
        let mut command = std::process::Command::new("reg.exe");
        match operation {
            RegistryOperation::SetDefaultValue { key, value } => {
                command.args([
                    "add",
                    &format!("{}\\{key}", hive.reg_name()),
                    "/ve",
                    "/d",
                    &value,
                    "/f",
                ]);
            }
            RegistryOperation::SetValue { key, name, value } => {
                command.args([
                    "add",
                    &format!("{}\\{key}", hive.reg_name()),
                    "/v",
                    &name,
                    "/d",
                    &value,
                    "/f",
                ]);
            }
            RegistryOperation::DeleteValue { key, name } => {
                command.args([
                    "delete",
                    &format!("{}\\{key}", hive.reg_name()),
                    "/v",
                    &name,
                    "/f",
                ]);
            }
            RegistryOperation::DeleteKey { key } => {
                command.args(["delete", &format!("{}\\{key}", hive.reg_name()), "/f"]);
            }
        }
        let output = command.output().map_err(|error| {
            format!("Could not update thumbnail-provider registration: {error}")
        })?;
        // Deletion is idempotent: absence is the desired state during disable.
        if !output.status.success() && !is_deletion {
            return Err(format!(
                "Could not update thumbnail-provider registration: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_associates_the_com_class_and_sync_root_without_file_type_handlers() {
        let operations =
            registration_operations(Path::new(r"C:\\Lymic\\resources\\lymic-shell-ext.dll"));

        assert_eq!(
            operations,
            vec![
                RegistryOperation::SetDefaultValue {
                    key: format!("Software\\Classes\\CLSID\\{THUMBNAIL_PROVIDER_CLSID}"),
                    value: "Lymic Thumbnail Provider".to_string(),
                },
                RegistryOperation::SetDefaultValue {
                    key: format!("Software\\Classes\\CLSID\\{THUMBNAIL_PROVIDER_CLSID}\\InprocServer32"),
                    value: r"C:\\Lymic\\resources\\lymic-shell-ext.dll".to_string(),
                },
                RegistryOperation::SetValue {
                    key: format!("Software\\Classes\\CLSID\\{THUMBNAIL_PROVIDER_CLSID}\\InprocServer32"),
                    name: "ThreadingModel".to_string(),
                    value: "Apartment".to_string(),
                },
                RegistryOperation::SetValue {
                    key: format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\SyncRootManager\\{PROVIDER_ID}"),
                    name: "ThumbnailProvider".to_string(),
                    value: THUMBNAIL_PROVIDER_CLSID.to_string(),
                },
            ]
        );
    }

    #[test]
    fn unregistering_only_removes_lymics_association_and_com_class() {
        assert_eq!(
            unregistration_operations(),
            vec![
                RegistryOperation::DeleteValue {
                    key: format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\SyncRootManager\\{PROVIDER_ID}"),
                    name: "ThumbnailProvider".to_string(),
                },
                RegistryOperation::DeleteKey {
                    key: format!("Software\\Classes\\CLSID\\{THUMBNAIL_PROVIDER_CLSID}"),
                },
            ]
        );
    }
}
