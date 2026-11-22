//! Autostart: starting Lymic when the user signs in to Windows.
//!
//! Two mechanisms exist and the backend picks one at runtime (never the frontend):
//!
//! * **Login item** (`tauri-plugin-autostart`): on Windows it writes
//!   `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Used for every unpackaged run (GitHub/NSIS
//!   installs, `tauri dev`) and on macOS and Linux.
//! * **StartupTask** (`Windows.ApplicationModel.StartupTask`, declared in the MSIX manifest as
//!   `windows.startupTask`): used when the process has Package Identity.
//!
//! Why a packaged run cannot use the Run key: Microsoft documents that for a packaged desktop app
//! "all writes under HKCU are copied on write to a private per-user, per-app location"
//! (learn.microsoft.com, "Understanding how packaged desktop apps run on Windows", Registry
//! section). Explorer, which starts the Run entries at sign-in, reads the real hive, so the entry the
//! plugin writes is not expected to start the app. Microsoft's documented mechanism for a packaged
//! desktop app is the `windows.startupTask` extension plus the `StartupTask` API (learn.microsoft.com,
//! "StartupTask Class"). **Not verified on a registered package**: the build machine could not
//! register one. See `docs/store/local-msix-build.md` for the checklist.
//!
//! The decisions that do not need Windows (what a state means, what an enable request that did not
//! take effect should tell the user, when a launch is a silent background start) are plain functions
//! and are tested here on every platform. The WinRT calls are a thin adapter behind
//! [`StartupTaskPort`].

use std::ffi::{OsStr, OsString};

/// Argument the login-item mechanism passes to the app so that it starts hidden in the tray.
pub(crate) const AUTOSTART_ARGUMENT: &str = "--autostart";

/// The `TaskId` of the `desktop:StartupTask` in `src-tauri/msix/AppxManifest.template.xml`. Both must
/// agree; `manifest_declares_the_startup_task_the_app_toggles` pins that.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const STARTUP_TASK_ID: &str = "LymicStartup";

/// The states of `Windows.ApplicationModel.StartupTaskState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupTaskState {
    Disabled,
    DisabledByUser,
    Enabled,
    DisabledByPolicy,
    EnabledByPolicy,
    /// A value a newer Windows may add. Never treated as enabled.
    Unknown(i32),
}

impl StartupTaskState {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Disabled,
            1 => Self::DisabledByUser,
            2 => Self::Enabled,
            3 => Self::DisabledByPolicy,
            4 => Self::EnabledByPolicy,
            other => Self::Unknown(other),
        }
    }

    pub(crate) fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled | Self::EnabledByPolicy)
    }
}

/// The StartupTask API of the current package. Implemented over WinRT on Windows; faked in tests.
pub(crate) trait StartupTaskPort {
    fn state(&self) -> Result<StartupTaskState, String>;
    /// `StartupTask.RequestEnableAsync`. A packaged desktop app gets no consent dialog. It does not
    /// override a choice the user made in Task Manager or Settings.
    fn request_enable(&self) -> Result<StartupTaskState, String>;
    fn disable(&self) -> Result<(), String>;
}

/// What the user should do when Windows kept autostart off after an enable request.
fn enable_refusal(state: StartupTaskState) -> String {
    match state {
        StartupTaskState::DisabledByUser => "Windows is keeping Lymic from starting at sign-in because it was turned off in Windows. Turn it back on in Windows Settings under Apps, Startup, or in the Startup apps tab of Task Manager.".to_string(),
        StartupTaskState::DisabledByPolicy => "Starting Lymic at sign-in is disabled by a Windows policy on this device.".to_string(),
        other => format!("Windows did not enable starting Lymic at sign-in (state: {other:?})."),
    }
}

pub(crate) fn startup_task_is_enabled(port: &impl StartupTaskPort) -> Result<bool, String> {
    port.state().map(StartupTaskState::is_enabled)
}

/// Switches the startup task. The outcome is read back from Windows and never assumed: a request
/// that Windows declined is an error that says why, not a silent no-op.
pub(crate) fn set_startup_task_enabled(
    port: &impl StartupTaskPort,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        let state = port.request_enable()?;
        return if state.is_enabled() {
            Ok(())
        } else {
            Err(enable_refusal(state))
        };
    }
    port.disable()?;
    match port.state()? {
        StartupTaskState::EnabledByPolicy => {
            Err("Starting Lymic at sign-in is enforced by a Windows policy on this device and cannot be turned off here.".to_string())
        }
        _ => Ok(()),
    }
}

/// Whether this launch should keep the window hidden and run in the tray.
///
/// The login item passes [`AUTOSTART_ARGUMENT`]. A StartupTask launch carries no arguments; it is
/// recognised by its activation kind instead (`started_by_startup_task`). Without stored credentials
/// the window is always shown, because there is nothing to run in the background.
pub(crate) fn should_start_in_background(
    args: &[OsString],
    has_credentials: bool,
    started_by_startup_task: bool,
) -> bool {
    has_credentials
        && (started_by_startup_task || args.iter().any(|arg| arg == OsStr::new(AUTOSTART_ARGUMENT)))
}

#[cfg(windows)]
mod winrt {
    use super::{StartupTaskPort, StartupTaskState, STARTUP_TASK_ID};
    use windows::{
        core::HSTRING,
        ApplicationModel::{
            Activation::ActivationKind, AppInstance, StartupTask,
            StartupTaskState as WinRtStartupTaskState,
        },
    };

    fn describe(error: windows::core::Error) -> String {
        format!("The Windows startup task could not be used: {error}")
    }

    fn state_of(state: WinRtStartupTaskState) -> StartupTaskState {
        StartupTaskState::from_code(state.0)
    }

    /// The StartupTask declared in this package's manifest. WinRT agile objects, so any thread works;
    /// callers run it on a blocking thread because `join` waits for the asynchronous operation.
    pub(crate) struct PackagedStartupTask;

    impl PackagedStartupTask {
        fn task(&self) -> Result<StartupTask, String> {
            StartupTask::GetAsync(&HSTRING::from(STARTUP_TASK_ID))
                .and_then(|operation| operation.join())
                .map_err(describe)
        }
    }

    impl StartupTaskPort for PackagedStartupTask {
        fn state(&self) -> Result<StartupTaskState, String> {
            self.task()?.State().map(state_of).map_err(describe)
        }

        fn request_enable(&self) -> Result<StartupTaskState, String> {
            self.task()?
                .RequestEnableAsync()
                .and_then(|operation| operation.join())
                .map(state_of)
                .map_err(describe)
        }

        fn disable(&self) -> Result<(), String> {
            self.task()?.Disable().map_err(describe)
        }
    }

    /// Whether Windows started this process through the package's startup task. Never errors:
    /// when the activation cannot be read, the window is simply shown (the safe outcome).
    ///
    /// For packaged apps `GetActivatedEventArgs` returns the arguments only on its first call
    /// (Microsoft documentation), so this must be called once and early.
    pub(crate) fn launched_by_startup_task() -> bool {
        AppInstance::GetActivatedEventArgs()
            .and_then(|args| args.Kind())
            .map(|kind| kind == ActivationKind::StartupTask)
            .unwrap_or(false)
    }
}

#[cfg(windows)]
pub(crate) use winrt::PackagedStartupTask;

/// Whether this launch came from the package's startup task. Always false when the process has no
/// Package Identity, and outside Windows.
#[cfg(windows)]
pub(crate) fn launched_by_startup_task() -> bool {
    super::package_identity::has_package_identity() && winrt::launched_by_startup_task()
}

#[cfg(not(windows))]
pub(crate) fn launched_by_startup_task() -> bool {
    false
}

/// The packaged mechanism for the current process. Blocking (it waits on WinRT operations): call it
/// from a blocking thread.
#[cfg(windows)]
pub(crate) fn packaged_autostart_enabled() -> Result<bool, String> {
    startup_task_is_enabled(&PackagedStartupTask)
}

#[cfg(windows)]
pub(crate) fn set_packaged_autostart(enabled: bool) -> Result<(), String> {
    set_startup_task_enabled(&PackagedStartupTask, enabled)
}

#[cfg(not(windows))]
const NO_STARTUP_TASK: &str = "The Windows startup task exists only on Windows.";

#[cfg(not(windows))]
pub(crate) fn packaged_autostart_enabled() -> Result<bool, String> {
    Err(NO_STARTUP_TASK.to_string())
}

#[cfg(not(windows))]
pub(crate) fn set_packaged_autostart(_enabled: bool) -> Result<(), String> {
    Err(NO_STARTUP_TASK.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Models Windows: a request is honoured unless the user disabled the task, `Disable` is
    /// ignored under a policy.
    struct FakeStartupTask {
        state: RefCell<StartupTaskState>,
        fails: bool,
    }

    impl FakeStartupTask {
        fn in_state(state: StartupTaskState) -> Self {
            Self {
                state: RefCell::new(state),
                fails: false,
            }
        }
    }

    impl StartupTaskPort for FakeStartupTask {
        fn state(&self) -> Result<StartupTaskState, String> {
            if self.fails {
                return Err("no package".to_string());
            }
            Ok(*self.state.borrow())
        }

        fn request_enable(&self) -> Result<StartupTaskState, String> {
            let mut state = self.state.borrow_mut();
            if *state == StartupTaskState::Disabled {
                *state = StartupTaskState::Enabled;
            }
            Ok(*state)
        }

        fn disable(&self) -> Result<(), String> {
            let mut state = self.state.borrow_mut();
            if *state == StartupTaskState::Enabled {
                *state = StartupTaskState::Disabled;
            }
            Ok(())
        }
    }

    fn args(extra: &[&str]) -> Vec<OsString> {
        std::iter::once("lymic")
            .chain(extra.iter().copied())
            .map(OsString::from)
            .collect()
    }

    #[test]
    fn windows_state_codes_map_to_states_and_only_the_enabled_ones_count_as_on() {
        let states = [
            (0, StartupTaskState::Disabled, false),
            (1, StartupTaskState::DisabledByUser, false),
            (2, StartupTaskState::Enabled, true),
            (3, StartupTaskState::DisabledByPolicy, false),
            (4, StartupTaskState::EnabledByPolicy, true),
            (99, StartupTaskState::Unknown(99), false),
            (-1, StartupTaskState::Unknown(-1), false),
        ];
        for (code, state, enabled) in states {
            assert_eq!(StartupTaskState::from_code(code), state, "code {code}");
            assert_eq!(state.is_enabled(), enabled, "{state:?}");
        }
    }

    #[test]
    fn the_startup_task_reports_what_windows_says() {
        assert!(
            !startup_task_is_enabled(&FakeStartupTask::in_state(StartupTaskState::Disabled))
                .unwrap()
        );
        assert!(
            startup_task_is_enabled(&FakeStartupTask::in_state(StartupTaskState::Enabled)).unwrap()
        );
        assert!(startup_task_is_enabled(&FakeStartupTask {
            state: RefCell::new(StartupTaskState::Enabled),
            fails: true
        })
        .is_err());
    }

    #[test]
    fn enabling_and_disabling_round_trip_through_the_startup_task() {
        let task = FakeStartupTask::in_state(StartupTaskState::Disabled);

        set_startup_task_enabled(&task, true).unwrap();
        assert!(startup_task_is_enabled(&task).unwrap());

        set_startup_task_enabled(&task, false).unwrap();
        assert!(!startup_task_is_enabled(&task).unwrap());
    }

    #[test]
    fn enabling_after_the_user_turned_it_off_in_windows_is_an_error_that_says_where_to_turn_it_on()
    {
        let task = FakeStartupTask::in_state(StartupTaskState::DisabledByUser);

        let error = set_startup_task_enabled(&task, true).unwrap_err();

        assert!(error.contains("Windows Settings"), "{error}");
        assert!(error.contains("Task Manager"), "{error}");
        assert!(!startup_task_is_enabled(&task).unwrap());
    }

    #[test]
    fn a_policy_that_blocks_or_enforces_autostart_is_reported_not_ignored() {
        let blocked = FakeStartupTask::in_state(StartupTaskState::DisabledByPolicy);
        assert!(set_startup_task_enabled(&blocked, true)
            .unwrap_err()
            .contains("policy"));

        let enforced = FakeStartupTask::in_state(StartupTaskState::EnabledByPolicy);
        assert!(set_startup_task_enabled(&enforced, false)
            .unwrap_err()
            .contains("policy"));
        assert!(startup_task_is_enabled(&enforced).unwrap());
    }

    #[test]
    fn disabling_an_already_disabled_task_succeeds() {
        let task = FakeStartupTask::in_state(StartupTaskState::DisabledByUser);

        assert!(set_startup_task_enabled(&task, false).is_ok());
    }

    #[test]
    fn a_failing_windows_call_is_surfaced() {
        let task = FakeStartupTask {
            state: RefCell::new(StartupTaskState::Disabled),
            fails: true,
        };

        assert!(set_startup_task_enabled(&task, false).is_err());
    }

    #[test]
    fn login_item_launch_with_credentials_starts_in_background() {
        assert!(should_start_in_background(
            &args(&[AUTOSTART_ARGUMENT]),
            true,
            false
        ));
    }

    #[test]
    fn startup_task_launch_with_credentials_starts_in_background_without_any_argument() {
        assert!(should_start_in_background(&args(&[]), true, true));
    }

    #[test]
    fn a_launch_without_credentials_always_shows_the_window() {
        assert!(!should_start_in_background(
            &args(&[AUTOSTART_ARGUMENT]),
            false,
            false
        ));
        assert!(!should_start_in_background(&args(&[]), false, true));
    }

    #[test]
    fn a_manual_launch_shows_the_window() {
        assert!(!should_start_in_background(&args(&[]), true, false));
    }

    #[test]
    fn test_binaries_were_not_started_by_a_startup_task() {
        assert!(!launched_by_startup_task());
    }

    #[test]
    fn manifest_declares_the_startup_task_the_app_toggles() {
        let manifest = include_str!("../../msix/AppxManifest.template.xml");

        assert!(manifest.contains("Category=\"windows.startupTask\""));
        assert!(manifest.contains(&format!("TaskId=\"{STARTUP_TASK_ID}\"")));
        // Off until the user turns it on: the same default as the unpackaged login item.
        assert!(manifest.contains("Enabled=\"false\""));
        assert!(manifest.contains("EntryPoint=\"Windows.FullTrustApplication\""));
    }

    /// The only part of the WinRT adapter that can run here: a test binary has no Package Identity,
    /// so Windows must answer with an error, not a crash, and the packaged path is never taken.
    #[cfg(windows)]
    #[test]
    fn the_winrt_startup_task_fails_cleanly_without_package_identity() {
        let error = PackagedStartupTask.state().unwrap_err();

        assert!(error.contains("startup task"), "{error}");
        assert!(packaged_autostart_enabled().is_err());
    }
}
