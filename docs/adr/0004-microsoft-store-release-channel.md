# Separate Microsoft Store release channel

Lymic will continue to distribute the existing Windows NSIS installer through GitHub, including its Tauri updater, while distributing a separate x64 MSIX package through the Microsoft Store. The Store package is updated only by the Store and has no GitHub/Tauri updater path; this avoids competing updaters and lets the Store provide hosting and package signing. The first Store release is a private Flight, requires reliable persisted Watched Folder access, and does not migrate configuration or Queued Synchronizations from an existing NSIS installation.

## Considered Options

- List the existing NSIS installer in the Microsoft Store. Rejected because it requires publisher code signing and leaves installation updates outside the Store.
- Replace the GitHub installer with MSIX for every Windows user. Rejected for the first release because current installations use the GitHub/NSIS updater and an automatic migration risks local configuration and queued work.
