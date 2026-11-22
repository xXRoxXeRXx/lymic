# Virtual Library manual verification record

**Status: NOT YET RUN.** Every result below is empty on purpose.

This is the manual verification record for the Windows adapter tracer bullet ([#278](https://github.com/xXRoxXeRXx/lymic/issues/278), part of [#266](https://github.com/xXRoxXeRXx/lymic/issues/266), [ADR-0003](adr/0003-windows-virtual-cloud-library.md)): enabling the Virtual Library, registering the sync root, browsing the projected tree in Explorer, hydrating an original, and refusing a folder inside another sync root. The cleanup on disable and logout ([#283](https://github.com/xXRoxXeRXx/lymic/issues/283)) has its own section below; it is also **not yet run**.

## What was and was not observed

The code for this flow was written and its platform-neutral parts are covered by unit tests (`cargo test`: placeholder mapping, identity encoding, nested-root decision with a fake operating system, block alignment, population and hydration orchestration with fakes, the settings commands with a fake service, the streaming original download against a local fake server). The Windows calls themselves (`src-tauri/src/cloud_files/adapter/`: `StorageProviderSyncRootManager.Register`, `CfConnectSyncRoot`, the `FETCH_PLACEHOLDERS` and `FETCH_DATA` callbacks, `CfGetSyncRootInfoByPath`, the registry read) compile but **cannot be unit tested**, and **nothing was observed in Explorer by the author of this change**. The machine it was written on is Windows Server 2022 without Developer Mode and cannot register an MSIX package, so no packaged run, no registration and no callback has ever executed. Do not read any acceptance criterion of #278 as verified until a person has run this checklist and filled in the results.

The behaviour of the Windows API that the design leans on (registration and `CfConnectSyncRoot` work, listing a folder fires `FETCH_PLACEHOLDERS` for that folder only and hydrates nothing, opening a file fires `FETCH_DATA`) was observed by the maintainer in the gate prototype (`vl-gates-prototype`), not in this code.

## Environment to record

| Item | Value |
| --- | --- |
| Windows edition and build | |
| Local MSIX registered per [`docs/store/local-msix-build.md`](store/local-msix-build.md) (identity name, version) | |
| Immich server version | |
| Lymic commit | |
| Pictures folder redirected into OneDrive on this machine | |
| Date and who ran it | |

## Checklist

Fill the **Result** column with PASS, FAIL or the observed behaviour, and add a note for anything unexpected.

| # | Step | Expected | Result |
| --- | --- | --- | --- |
| 1 | Run the unpackaged executable (`npm run tauri dev`, or `lymic-desktop.exe` from `target`). Open Settings. | No Virtual Library section. | |
| 2 | Start the registered package from its Start menu entry. Open Settings. | The Virtual Library section is shown, disabled, with the folder `<Pictures>\Lymic` (Known Folder, also when Pictures is redirected). | |
| 3 | Make sure the folder `<Pictures>\Lymic` is not inside another sync root. Switch the toggle on. | No error. The folder is created. The toggle stays on after restarting Lymic. | |
| 4 | Check the registration: `reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager" /s` | An entry `Lymic.VirtualLibrary!<your SID>!Library` whose `UserSyncRoots` value is the folder. | |
| 5 | Open the folder in Explorer. | It shows `All Photos` and `Albums` with cloud status icons, and nothing else. The folder is listed in the Explorer navigation pane as a sync root. | |
| 6 | Before opening any subfolder, note the request count in the Immich access log (or a proxy). Open `All Photos`. | The years appear. Only the requests for that listing (`timeline/buckets`) were made; no month and no album was listed. | |
| 7 | Open a year, then a month. | The months, then the files appear. Names, sizes and dates match the Immich web app. The month is the capture's local month. Only that month was searched. | |
| 8 | Find a Live Photo and a name collision in a month or album. | The Live Photo is two files with one base name (`IMG_0001.HEIC` and `IMG_0001.MOV`). A collision shows the ` (<6 characters>)` suffix on every colliding name. | |
| 9 | Open `All Photos\Undated` and `Albums`, then one album. | Assets without a capture time are under `Undated`. Owned and shared albums appear as folders; an album folder shows its assets. | |
| 10 | In a month view with Large icons (thumbnails on), watch the Immich access log and the file status icons. | Record whether Explorer reads file content to draw thumbnails. There is no thumbnail provider yet (separate ticket); any `assets/<id>/original` request caused only by browsing is a finding, not a pass. | |
| 11 | Open a placeholder (for example in Photos or Paint). | The file opens. Its status icon changes to locally available. | |
| 12 | Copy a hydrated file elsewhere and compare its SHA-256 with the original downloaded from the Immich web app. | Identical. | |
| 13 | Open a large video (more than 1 GiB if possible). Watch Lymic's memory in Task Manager. | The video plays or copies completely. Memory does not grow with the file size. | |
| 14 | Open an album or month with more than 1000 assets. | All assets appear (placeholders are handed over in batches of 1000). | |
| 15 | Switch the toggle off (confirm the dialog), then on again for the same folder. | Re-enabling is not mistaken for a nested root and works. | |
| 16 | Quit Lymic and start it again with the toggle on. Without touching Settings, open an unhydrated placeholder. | It hydrates (the sync root is served again on app start). | |
| 17 | Switch the toggle off and confirm (see steps 28 to 43 for the cleanup in detail). Look at the folder. | The Virtual Library folder no longer shows unhydrated placeholders: nothing is left to open. | |
| 18 | While enabled, try Change folder. | Refused with a localized message asking to disable the Virtual Library first. | |
| 19 | Disable the toggle. Choose, with Change folder, a folder inside OneDrive (or inside any other cloud sync folder), then switch on. | Refused with a localized message that asks for another folder and names the other sync root. The toggle stays off. No new entry appears under `SyncRootManager` and the folder is not created. | |
| 20 | Choose a folder that contains another sync root (for example your user profile folder, which contains `OneDrive`). Switch on. | Refused the same way. Nothing is registered. | |
| 21 | Repeat 19 in at least German and Arabic (language setting). | The message is shown in the chosen language, the path is readable, and the right-to-left text is not garbled. | |
| 22 | While the Virtual Library is enabled, stop the Immich server (or block it) and open an unhydrated placeholder. | The read fails with a network error and does not hang or retry in a loop. | |
| 23 | Restore the server, then give Lymic an invalid API key (log in with a revoked key) and open an unhydrated placeholder. | The read fails with an authentication error; no loop. | |
| 24 | Open a folder you have not browsed while the server is stopped, then restore the server and open it again. | The first listing fails; the second one works (a failed listing leaves the folder unpopulated). | |
| 25 | Inspect the Immich access log for the whole session. | Only `GET` requests (plus the `POST` metadata searches). No write to any asset. | |
| 26 | Inspect `AppxManifest.xml` in the registered package. | No `windows.cloudFiles` extension and no COM server declaration (the gate prototype's `Register` failed with 0x80070490 with one). | |
| 27 | On a machine where the nested-root check can be exercised only through the registry (or only through `CfGetSyncRootInfoByPath`), note which one caught it, if it can be told. | Record. Reading `HKLM\...\SyncRootManager` from inside the package has not been confirmed to see other providers' roots. | |
| H1 | **Hydration failures (#281), NOT YET RUN.** With the Virtual Library on and Lymic open, stop (or block) the Immich server. Open an unhydrated placeholder in Photos or Paint, and time it. | The open fails within about 15 seconds (connect and response timeout), with a network-style error ("network not connected" or the cloud provider "not available" message); Explorer does not hang. Immich's access log (or a proxy) shows exactly one attempt for the open: no retry loop. The shell status code is `STATUS_CLOUD_FILE_NETWORK_UNAVAILABLE`. | |
| H2 | Same situation: Lymic window open, on any view. | Exactly one notice appears (a banner above the content, and one activity log entry: "Could not open ... because Immich could not be reached."). It has no "Sign in again" button. | |
| H3 | Still offline: open ten different unhydrated placeholders (or select ten and press Enter), then open the same one again several times. | Every open fails promptly, but no second notice and no second log entry appear. | |
| H4 | Dismiss the banner, restore the server, open a placeholder (it hydrates), stop the server again and open another one. | A new notice appears: a success ended the episode. Also, with the server offline for longer than five minutes, one more notice appears at the next failure after the five quiet minutes. | |
| H5 | Restore the server. Revoke the API key in Immich (or log in with a revoked key), then open an unhydrated placeholder. | The open fails promptly with an authentication error (`STATUS_CLOUD_FILE_AUTHENTICATION_FAILED`), no retry loop. One notice appears: "... your Immich sign-in is no longer valid", with a "Sign in again" button. Record the exact Explorer or app error text. | |
| H6 | Press "Sign in again". | Lymic shows the login form for the same server with the explanation and a "Not now" button. Nothing was logged out: the Virtual Library stays enabled and its folder is untouched. Enter a new valid API key: the form closes, Lymic is signed in, and opening the same placeholder now hydrates. Repeat once and press "Not now": the form closes and nothing changes. | |
| H7 | Delete (or move to the Immich trash and empty it) a photo in the Immich web app, without refreshing the Virtual Library. Open its (unhydrated) placeholder in Explorer. | The open fails promptly with a "file not found" style error (record the actual Explorer or app text). One notice appears naming the file, saying it was removed from Immich. **Open question to record:** whether the platform accepts `STATUS_OBJECT_NAME_NOT_FOUND` as the completion status (Explorer shows an error and the read ends) or substitutes another error; if it misbehaves, the one-line fallback is `STATUS_CLOUD_FILE_INVALID_REQUEST` in `ntstatus` in `adapter/callbacks.rs`. | |
| H8 | After H7, wait for the next reconciliation (#280, when it has landed) or press Refresh Library. | The placeholder is gone from the folder. (Until #280 lands this is expected to fail: record "not applicable".) | |
| H9 | Start opening a large video (more than 1 GiB) so that it downloads, then close the program that opened it (or cancel the copy in Explorer) after a few seconds. Watch Immich's access log and Lymic's network use. | The download stops within a moment (the connection to Immich closes). Lymic's standard error shows "a download was cancelled by the reader and stopped". No notice appears. If instead the note "a cancel arrived for a download that is not (or no longer) in flight" shows and the download continues, the cancel callback's transfer key does not match the fetch's: record it. | |
| H10 | Take Immich offline in the middle of a large download (the `FETCH_DATA` is running). | The read fails within about 30 seconds (no data for 30 seconds) with a network error; one notice. The partly hydrated file is not marked as complete. | |
| H11 | Switch Lymic's language to German and Arabic and trigger the notices of H2, H5 and H7 once each. | The banner, the log entry, "Sign in again", "Not now" and the explanation on the login form are in the chosen language and read correctly. | |
| H12 | Open a folder you have not browsed while the server is stopped, then again while it is still stopped. | The listing fails each time; the notice is the same episode as in H2 (not a second one). | |

## Reconciliation and Refresh Library (#280): manual steps

**Status: NOT YET RUN.** Same conventions and environment as the checklist above; fill the Result column. The platform-neutral logic is unit tested (`cargo test`: `cloud_files::diff` for new, removed and changed placeholders, the safety rules, case-insensitive names and skipped names; `cloud_files::reconcile` for populated-only listing, parents first, failures that must not delete, removed directories; `cloud_files::refresh` for the single-flight guard and the schedule with a fake clock; `db::virtual_library_populated`; the `refresh_virtual_library` command with a fake service). The Windows calls in `src-tauri/src/cloud_files/adapter/reconcile.rs` (`FindFirstFileW`, `CfGetPlaceholderStateFromAttributeTag`, `CfGetPlaceholderInfo`, `CfCreatePlaceholders`, `CfOpenFileWithOplock`, `CfUpdatePlaceholder`, deleting a placeholder) and the `mark_populated` call in `FETCH_PLACEHOLDERS` compile but **have never run**; nothing below was observed.

Preparation: enable the Virtual Library on a fresh folder (see "Known gaps": directories populated by an earlier build are not recorded). Open `All Photos`, one year, one month with a few photos, `Albums` and one album, so they are populated. Leave one other album and one other month unopened.

| # | Step | Expected | Result |
| --- | --- | --- | --- |
| R1 | Inspect the database: `sqlite3 <app data>\lymic.db "select path from virtual_library_populated_directories"` (the app data folder of the installed package). | One row per directory you opened (`""` for the root, `All Photos`, `All Photos/<year>`, `All Photos/<year>/<MM Month>`, `Albums`, `Albums/<album>`) and none for the ones you did not open. A directory whose listing failed (step 24 above) has no row. | |
| R2 | Upload a new photo to Immich (web app) so it belongs to the opened month and the opened album. Open Settings, Virtual Library, **Refresh Library**. | The button shows "Refreshing..." and returns. The new placeholder appears in the open month and album (F5 in Explorer if needed) with the right name, size and dates. | |
| R3 | In Immich, remove that photo from the album only. Refresh Library. | It disappears from the album folder but stays in the month (`All Photos`). | |
| R4 | In Immich, move a photo to the trash. Refresh Library. | The placeholder is gone from its month and from every album. Explorer shows no error and the Recycle Bin did not receive it. | |
| R5 | Hydrate a photo (open it), then change it in Immich so its size changes (for example replace the asset). Refresh Library. | The placeholder shows the new size; the local copy is dropped (status icon returns to cloud only) and opening it downloads the new original. | |
| R6 | Hydrate a photo, then modify it locally so it is no longer in sync (the status icon shows pending sync), and remove that asset in Immich. Refresh Library. | The modified file is not deleted. | |
| R7 | Put your own file (`notes.txt`) into an opened month folder and a folder `Mine` (with a file in it) into `Albums`. Refresh Library. | Both survive. No upload happens. | |
| R8 | Delete an Immich album that is opened (populated) and contains a hydrated photo. Refresh Library. | The album folder disappears together with its placeholders; its row is gone from `virtual_library_populated_directories`. Repeat with a `notes.txt` placed inside the album folder first: the folder and `notes.txt` stay, the placeholders go, and the row stays. | |
| R9 | Remove in Immich the album you left unopened, and add a photo to a month you never opened. Refresh Library and watch the Immich access log. | Neither the unopened album nor the unopened month was listed during the refresh (no `search/metadata` request for them). The month is listed, with current content, the first time you open it. | |
| R10 | Stop the Immich server. Refresh Library. | An error naming the Virtual Library refresh with the reason is shown. Nothing in the Virtual Library disappeared. Start the server and refresh again: it works. | |
| R11 | Revoke the API key (or log out of Lymic) and refresh. | An error is shown; nothing disappeared. | |
| R12 | Quit Lymic. Add a photo in Immich. Start Lymic and wait about a minute without touching Settings. | The photo appears in the opened folder: the startup run works. Note how long it took. | |
| R13 | Add another photo in Immich and wait 10 minutes with Lymic running in the tray. | It appears without any action: the timer works. | |
| R14 | Make a run slow (a large Library or a throttled connection). Press Refresh Library twice while it runs, and let the startup or timer run fall into it. | Only one run is active at any time (the Immich access log shows no two listings at once). The presses wait and return after one follow-up run. | |
| R15 | Hydrate and pin ("Always keep on this device") a photo, remove it in Immich and refresh. | Record what happens. Expected: an in-sync pinned placeholder is deleted like any other. | |
| R16 | Remove a photo in Immich and refresh (the veto of #279 is active). In the Lymic log (standard error) look for `refused a delete`. | The removal works: `on_notify_delete` acknowledges a delete for which `adapter::reconcile::is_own_removal(path)` is true instead of refusing it. If the placeholder stays and the log says `refused a delete` for it, the registration of Lymic's own deletion does not reach the callback (check path spelling); if no `NOTIFY_DELETE` arrives at all for Lymic's own delete, the exception is unneeded. Also try deleting the same kind of placeholder in Explorer while no refresh runs: still refused. | |
| R17 | Change the Virtual Library folder (disable, choose another folder, enable) and open a directory. | `virtual_library_populated_directories` holds only directories of the new folder. | |

Things to look for beyond the table: whether `CfOpenFileWithOplock` works on a directory placeholder to read its identity (a directory that cannot be read counts as not ours and is left, which would keep a removed album's folder forever); whether reading the identity of thousands of placeholders per run is fast enough (a month with a few thousand photos); whether `CF_UPDATE_FLAG_DEHYDRATE` together with a new size works on a hydrated placeholder; whether `remove_dir` on a never-populated directory placeholder triggers `FETCH_PLACEHOLDERS` (it should not); whether Lymic's own deletion of a placeholder reaches `NOTIFY_DELETE` at all.

## Cleanup on disable and logout (#283): NOT YET RUN

The cleanup ([#283](https://github.com/xXRoxXeRXx/lymic/issues/283)) is implemented the same way as the rest of the adapter: the decisions (what is Lymic's, what stays, the order, a failure half way, running twice) are unit tested in `src-tauri/src/cloud_files/cleanup.rs` and `commands/virtual_library.rs` with a fake filesystem and a fake host, and the confirmation and notice flow is tested in the controllers. The Windows calls (`adapter/teardown.rs`: `CfGetPlaceholderInfo` on an attributes-only handle, `StorageProviderSyncRootManager.Unregister`, `CfDisconnectSyncRoot`) **cannot be unit tested and have never run**: this machine cannot register an MSIX package or open Explorer. Everything below is expected behaviour to be checked, not an observation. Use a throw-away Immich account and a Virtual Library folder with nothing valuable in it.

What the cleanup assumes about Windows, each to be confirmed by the steps:

- A hydrated or pinned original is still reported as a placeholder by `CfGetPlaceholderInfo`, with the identity Lymic stored and the in-sync state (steps 30 and 31).
- Reading that state does not hydrate anything and does not call Immich (step 33).
- A file the user created in the folder is not a placeholder (steps 30 and 31).
- Unregistering and deleting work while nothing serves the folder (steps 31 and 36).
- What `Unregister` does to placeholders is **not established**: the gate prototype saw unhydrated placeholders disappear and hydrated ones turn into plain files, unpackaged and on Windows Server only. The cleanup scans before it unregisters, so the outcome only decides whether Lymic or Windows removes the files. Record which (step 31).

| # | Step | Expected | Result |
| --- | --- | --- | --- |
| 28 | Enable the Virtual Library. Browse `All Photos` to a month, open (hydrate) two photos, and on a third use "Always keep on this device". Put a text file `mine.txt` into a month folder, and a text file `top.txt` into the root folder. Make a folder `Mine` in the root with a file inside. Edit one hydrated photo and save it in place. | The two photos show the locally available icon, the third shows the pinned icon. | |
| 29 | Switch the toggle off and decline the confirmation. | The dialog names that hydrated and pinned originals are removed and that files that are not placeholders and were not created by Lymic stay in place together with their folder. After declining, nothing changed: the toggle is still on, the folder is still served, files still open. | |
| 30 | Switch the toggle off and confirm. | The toggle turns off. A notice names the folder and lists `mine.txt`, `top.txt`, `Mine` (or its file) and the edited photo as kept. | |
| 31 | Inspect the folder in Explorer and with `reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager" /s`. Also note, if you can tell, whether Lymic or Windows removed the files (watch with Process Monitor for `DeleteFile` from `lymic-desktop.exe`). | The `Lymic.VirtualLibrary!<SID>!Library` entry is gone. The unhydrated, hydrated and pinned placeholders and the empty Lymic folders are gone. `mine.txt` with its month folder and the folders above it, `top.txt` with the root folder, `Mine` with its file, and the edited photo with its folders are all still there. Record which of the two removed the hydrated files. | |
| 32 | Open the kept files. | They open and have their content (nothing of the user's was deleted or truncated). | |
| 33 | During step 30, watch the Immich access log. | No request for any original and no listing: the cleanup reads only local placeholder state. | |
| 34 | Repeat 28 and 30 with no user files, no edited photo and no `Mine` folder. | No notice. The root folder is removed too. | |
| 35 | After 34, switch on again, then off again at once (nothing browsed). | Both work; no notice; the folder is created and removed again. | |
| 36 | Switch on, browse a few folders, then end `lymic-desktop.exe` in Task Manager (a crash: nothing connected). Start Lymic, switch off and confirm. | The cleanup completes the same way as in step 31 although the sync root was served by the previous process. | |
| 37 | Switch on, browse, switch off and confirm; then immediately repeat the same disable cleanup (switch on, off) | The second run finds nothing to remove and does not error. | |
| 38 | While the Virtual Library is on, lock one hydrated file (open it in another program so the delete is refused) and switch off. | A notice lists that item with the Windows reason and says to turn the Virtual Library on and off again. Everything else is removed and the sync root is unregistered. Close the other program, switch on, off: the item is removed. | |
| 39 | While the Virtual Library is on, log out: decline the confirmation. | The logout question also names that hydrated and pinned originals are removed and that other files stay. Declining changes nothing: still logged in, still served. | |
| 40 | Repeat 28, log out and confirm. | Same cleanup as 31 and the same notice as 30 (shown on the login screen). After logging in again the Virtual Library is off and the folder is not served. | |
| 41 | Log out while the Virtual Library is off. | The plain logout question only. Nothing is removed from the folder, which may be any folder the user already has. | |
| 42 | Repeat 30 in German and Arabic. | The dialogs and notices are in the chosen language; the right-to-left text and the path are not garbled. | |
| 43 | Open Task Manager details, switch the Virtual Library on and off five times. | Lymic's memory does not grow by a callback table per cycle (the table is now owned by the connection and freed on disconnect). | |

### Uninstall (gate #277): not observed

Nobody has observed what Windows leaves behind when the Store package is uninstalled, and the cleanup does not rely on it. Check on a machine that can: with the Virtual Library on, some photos hydrated and pinned and a user file in the folder, uninstall the package without switching the Virtual Library off, then record:

| Item | Observed |
| --- | --- |
| Is the `SyncRootManager` registration still there? | |
| Does the folder still show in Explorer with cloud icons? | |
| Do unhydrated placeholders open, fail, or stay? What error? | |
| Do hydrated and pinned files still open? | |
| Can the leftover folder be deleted in Explorer? Any error? | |
| After reinstalling, enabling and disabling the Virtual Library: is everything Lymic created removed? | |

Until this is observed, the guidance for a user who sees a leftover Lymic folder after an uninstall is: delete it in Explorer (placeholders and hydrated files can be deleted by the user; nothing is deleted in Immich), or reinstall Lymic, switch the Virtual Library on and off again so that its cleanup runs. The CHANGELOG says the same. An in-app note is not possible: Lymic is not running after an uninstall, and no uninstall hook exists.

## Read-only enforcement (#279)

**Status: NOT YET RUN.** Same environment table as above.

What was observed and what was not. In the gate prototype (`vl-gates-prototype`) the maintainer saw, on real Windows 11, Explorer Delete and Explorer Rename of a placeholder raise `NOTIFY_DELETE` and `NOTIFY_RENAME` from `explorer.exe`, and vetoed them with `STATUS_CLOUD_FILE_ACCESS_DENIED` through `CfExecute` `ACK_DELETE` and `ACK_RENAME`. That code was ported into `adapter/callbacks.rs` (`on_notify_delete`, `on_notify_rename`) and the refusal decision lives in `cloud_files/write_guard.rs`, unit tested. **Nothing below has been run against the ported code**, and these paths have **never been observed at all**, even in the prototype: Shift+Delete, dragging to the Recycle Bin, and creating a new local file in the folder. The author of this change cannot run Explorer or register a package (Windows Server 2022, no Developer Mode). Which callback each of the unobserved paths raises is unknown; the Lymic diagnostic output (standard error, lines starting `virtual library: refused a ...`) tells which callback fired and with which target.

Do the steps with the Virtual Library enabled and the Immich access log (or a proxy) open. Use a hydrated and an unhydrated placeholder, a file and a folder.

| # | Step | Expected | Result |
| --- | --- | --- | --- |
| R1 | Select a placeholder file in Explorer, press Delete (or right-click, Delete). | Refused with an error; the file stays. Lymic logs `refused a delete`. | |
| R2 | Same for a folder placeholder (for example a month, an album). | Refused; the folder and its content stay. | |
| R3 | Rename a file placeholder (F2). | Refused; the old name stays. Lymic logs `refused a rename inside the Virtual Library folder`. | |
| R4 | Rename a folder placeholder. | Refused. | |
| R5 | Press Shift+Delete on a placeholder (permanent delete, skips the Recycle Bin). Record which callback Lymic logged. | Refused; the file stays. If it is deleted, or no refusal is logged, that is a FAIL and a finding for the ADR gate 1. | |
| R6 | Drag a placeholder onto the Recycle Bin on the desktop. | Refused; Lymic logs `refused a rename into the Recycle Bin` or `refused a delete`. Nothing appears in the Recycle Bin. | |
| R7 | Drag a placeholder to another folder outside the Virtual Library (same volume), then to another drive. | Refused (move on the same volume is a rename, to another volume it is copy then delete). Record whether the copy is made and whether the delete is refused; a copy is the user's own file and is allowed. The placeholder must stay. | |
| R8 | Drag a placeholder to another folder inside the Virtual Library (for example from one month to another). | Refused. | |
| R9 | Cut a placeholder (Ctrl+X) and paste it elsewhere. | Refused; the placeholder stays. | |
| R10 | Select several placeholders and press Delete. | All refused; none disappears. | |
| R11 | Restore from the Recycle Bin, if something that was vetoed ever got there (only possible if R5-R7 failed). | Record. | |
| R12 | Right-click a hydrated file, Free up space. | Still works (dehydrate is not vetoed). | |
| R13 | Create a new file in the Virtual Library folder (for example New, Text Document, and copy a photo into the root and into `All Photos`). | The file is created and is not blocked. Lymic does not touch it and makes no request to Immich for it. | |
| R14 | Wait at least two minutes, then check the Immich access log and Lymic's activity log. | The files from R13 are never uploaded and never mentioned. No `POST /assets`, `PUT`, `PATCH` or `DELETE` of any kind. | |
| R15 | Delete or rename the file from R13 itself. | Record: is it refused or allowed? (It is not a placeholder, so no callback is expected, and it is allowed.) | |
| R16 | Try to add the Virtual Library folder (and a folder inside it, and its parent) as a Watched Folder; then add an ordinary Watched Folder and run a Synchronization. | The three are refused with the overlap message. The ordinary Watched Folder synchronizes as before and never reads the Virtual Library folder (check with Process Monitor on the folder if possible). | |
| R17 | Make a junction or symbolic link inside an ordinary Watched Folder that points at the Virtual Library folder (`mklink /J`), then synchronize. | Record whether the scanner enters it. The scanner does not follow symbolic links by design; junctions on Windows have not been tested. A scan that hydrates a placeholder is a FAIL. | |
| R18 | Inspect the Immich access log for the whole R session. | Only `GET` requests and the `POST` metadata searches. | |

Code-level checks done by reading and by unit tests (not by running Explorer): the projection and hydration make only `GET` requests and the read-only `POST /search/metadata` (`walking_the_whole_projection_sends_only_reads_to_immich`); nothing in `cloud_files` or `virtual_library` references the upload queue, Synchronization or Watched Folders; Watched Folder registration, enabling and moving the root all refuse an overlap, in both directions (`virtual_library_root`, `watched_folders/registration.rs`, `commands/virtual_library.rs`, each with tests).

## Known gaps this record will not close

- Delete and rename of placeholders are vetoed in code (#279) but have never been run against this code; see the section below. Do not read them as passing.
- Directories populated before reconciliation existed (#280) are not recorded as populated and are not reconciled; test reconciliation on a fresh enable.
- Hydration failures (#281): the Windows error path (completing `FETCH_DATA` with a failure status, the `CANCEL_FETCH_DATA` callback) has no unit test and is covered only by steps H1 to H12 above, which have not been run. What is unit tested: the classification of Immich failures, the failure to Cloud Files status mapping, the timeouts, the episode rule for notices, the cancel registry and the notice and re-login wiring in the frontend.
- Disabling and logout run the cleanup (#283); it has not run on Windows yet, see the section below. What Windows leaves behind after an MSIX uninstall (#277) is unknown.
- No thumbnails from Immich and no autostart hint yet; unhydrated placeholders work only while Lymic runs.
- Files whose size Immich does not report even after asking for the asset are left out of their directory (logged to standard error only).
