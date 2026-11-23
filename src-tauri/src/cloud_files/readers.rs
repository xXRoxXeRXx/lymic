//! Who is reading a placeholder, and which readers may cause Hydration (ADR-0003).
//!
//! A `FETCH_DATA` callback means some process asked for the bytes of a placeholder. Most of the
//! time that is the user opening a photo, which must work. Some background services also read every
//! file they find (Windows Search indexing and extracting properties, the out-of-process thumbnail
//! host) and would download the whole Library one original at a time. The adapter connects with
//! `CF_CONNECT_FLAG_REQUIRE_PROCESS_INFO` to learn the reader's image, logs every request with it,
//! and refuses the readers listed in [`judge`] without downloading anything.
//!
//! This is the platform-neutral half: no Windows type is involved, so the decision and the log line
//! are tested everywhere. The adapter fills [`Reader`] from `CF_CALLBACK_INFO.ProcessInfo`.

/// The process that asked for the data, as far as the platform said. Every field can be missing
/// (the platform does not always know), and a missing reader is never refused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Reader {
    pub process_id: u32,
    /// The executable of the process. The platform may spell it as a DOS path
    /// (`C:\Windows\System32\...`) or as a device path (`\Device\HarddiskVolume3\Windows\...`).
    pub image_path: Option<String>,
    /// The package family of a packaged caller, such as the Photos app.
    pub package_name: Option<String>,
}

/// What to do with a read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict {
    /// Serve it: a user, or a program the user started, wants the file.
    Allow,
    /// Fail it with `STATUS_CLOUD_FILE_UNSUCCESSFUL` and download nothing.
    Refuse(&'static str),
}

/// Windows services that read content on their own. Each is a system executable that nobody starts
/// to open a photo: the Search indexer and its two helper hosts, and `dllhost.exe`, the COM
/// surrogate that Explorer runs its out-of-process thumbnail extraction in (our own thumbnail
/// provider answers from a cache and never reads, so a read from here means a default handler
/// took over).
///
/// Not listed on purpose: `explorer.exe` (the user copying a file in Explorer reads through it),
/// anti-virus services (they skip offline files) and every user application.
const BACKGROUND_READERS: [(&str, &str); 4] = [
    (
        "searchprotocolhost.exe",
        "the Windows Search indexer reads files to index them",
    ),
    (
        "searchfilterhost.exe",
        "the Windows Search indexer reads files to index them",
    ),
    (
        "searchindexer.exe",
        "the Windows Search indexer reads files to index them",
    ),
    (
        "dllhost.exe",
        "the shell's thumbnail host reads files to draw thumbnails",
    ),
];

/// Only an executable in a Windows system directory counts, so a program of the user's that happens
/// to share a name is not refused.
const SYSTEM_DIRECTORIES: [&str; 2] = ["\\windows\\system32\\", "\\windows\\syswow64\\"];

/// Decides whether the read of `reader` may hydrate a placeholder.
pub(super) fn judge(reader: &Reader) -> Verdict {
    let Some(image) = reader.image_path.as_deref() else {
        return Verdict::Allow;
    };
    let image = image.replace('/', "\\").to_lowercase();
    let Some((directory, name)) = image.rsplit_once('\\') else {
        return Verdict::Allow;
    };
    // `directory` has no trailing separator; put it back so the match covers a whole component.
    let directory = format!("{directory}\\");
    if !SYSTEM_DIRECTORIES
        .iter()
        .any(|system| directory.ends_with(system))
    {
        return Verdict::Allow;
    }
    BACKGROUND_READERS
        .iter()
        .find(|(listed, _)| *listed == name)
        .map_or(Verdict::Allow, |(_, reason)| Verdict::Refuse(reason))
}

/// The line the audit log gets for one `FETCH_DATA`, with who asked, what and what was decided.
/// The path is the placeholder's path inside the Virtual Library.
pub(super) fn fetch_log_line(
    reader: &Reader,
    path: &str,
    offset: u64,
    length: u64,
    verdict: Verdict,
) -> String {
    let decision = match verdict {
        Verdict::Allow => "allowed".to_string(),
        Verdict::Refuse(reason) => format!("refused ({reason})"),
    };
    format!(
        "FETCH_DATA {decision}: '{path}' offset={offset} length={length} reader={}",
        describe(reader)
    )
}

/// `pid 4242 C:\...\app.exe (package Contoso.App)`, with what is unknown left out.
pub(super) fn describe(reader: &Reader) -> String {
    let mut text = format!("pid {}", reader.process_id);
    match reader.image_path.as_deref().filter(|path| !path.is_empty()) {
        Some(image) => {
            text.push(' ');
            text.push_str(image);
        }
        None => text.push_str(" (image unknown)"),
    }
    if let Some(package) = reader.package_name.as_deref().filter(|n| !n.is_empty()) {
        text.push_str(&format!(" (package {package})"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(image: &str) -> Reader {
        Reader {
            process_id: 4242,
            image_path: Some(image.to_string()),
            package_name: None,
        }
    }

    #[test]
    fn the_search_indexer_hosts_may_not_hydrate() {
        for image in [
            r"C:\Windows\System32\SearchProtocolHost.exe",
            r"C:\Windows\System32\SearchFilterHost.exe",
            r"C:\Windows\System32\SearchIndexer.exe",
        ] {
            assert!(
                matches!(judge(&reader(image)), Verdict::Refuse(_)),
                "{image}"
            );
        }
    }

    #[test]
    fn the_thumbnail_host_may_not_hydrate() {
        assert!(matches!(
            judge(&reader(r"C:\Windows\System32\dllhost.exe")),
            Verdict::Refuse(_)
        ));
    }

    #[test]
    fn the_platform_may_spell_the_image_as_a_device_path_in_any_case() {
        assert!(matches!(
            judge(&reader(
                r"\Device\HarddiskVolume3\WINDOWS\system32\searchprotocolhost.EXE"
            )),
            Verdict::Refuse(_)
        ));
        assert!(matches!(
            judge(&reader(r"C:\Windows\SysWOW64\SearchFilterHost.exe")),
            Verdict::Refuse(_)
        ));
    }

    #[test]
    fn the_user_copying_in_explorer_may_hydrate() {
        assert_eq!(judge(&reader(r"C:\Windows\explorer.exe")), Verdict::Allow);
    }

    #[test]
    fn a_photo_viewer_may_hydrate() {
        let mut photos =
            reader(r"C:\Program Files\WindowsApps\Microsoft.Windows.Photos\Photos.exe");
        photos.package_name = Some("Microsoft.Windows.Photos_8wekyb3d8bbwe".to_string());
        assert_eq!(judge(&photos), Verdict::Allow);
    }

    #[test]
    fn a_program_that_only_shares_a_name_with_a_service_is_not_refused() {
        assert_eq!(
            judge(&reader(r"C:\Users\me\Downloads\dllhost.exe")),
            Verdict::Allow
        );
        assert_eq!(
            judge(&reader(r"C:\Windows\System32x\SearchIndexer.exe")),
            Verdict::Allow
        );
    }

    #[test]
    fn a_reader_the_platform_could_not_name_is_never_refused() {
        assert_eq!(judge(&Reader::default()), Verdict::Allow);
        assert_eq!(judge(&reader("SearchIndexer.exe")), Verdict::Allow);
    }

    #[test]
    fn the_log_line_names_the_reader_the_file_the_range_and_the_decision() {
        let mut photos = reader(r"C:\Windows\explorer.exe");
        photos.package_name = Some("Contoso.App".to_string());

        let line = fetch_log_line(
            &photos,
            "All Photos/2025/a.jpg",
            4096,
            1_048_576,
            Verdict::Allow,
        );

        assert_eq!(
            line,
            "FETCH_DATA allowed: 'All Photos/2025/a.jpg' offset=4096 length=1048576 \
             reader=pid 4242 C:\\Windows\\explorer.exe (package Contoso.App)"
        );
    }

    #[test]
    fn a_refusal_in_the_log_says_why_and_an_unknown_image_is_marked() {
        let unknown = Reader {
            process_id: 7,
            ..Reader::default()
        };

        let line = fetch_log_line(&unknown, "a.jpg", 0, 10, Verdict::Refuse("because"));

        assert_eq!(
            line,
            "FETCH_DATA refused (because): 'a.jpg' offset=0 length=10 reader=pid 7 (image unknown)"
        );
    }
}
