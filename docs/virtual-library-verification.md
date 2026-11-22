# Virtual Library manual verification record

**Status: NOT YET RUN.** Every result below is empty on purpose.

This is the manual verification record for the Windows adapter tracer bullet ([#278](https://github.com/xXRoxXeRXx/lymic/issues/278), part of [#266](https://github.com/xXRoxXeRXx/lymic/issues/266), [ADR-0003](adr/0003-windows-virtual-cloud-library.md)): enabling the Virtual Library, registering the sync root, browsing the projected tree in Explorer, hydrating an original, and refusing a folder inside another sync root.

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
| 15 | Switch the toggle off, then on again for the same folder. | Re-enabling is not mistaken for a nested root and works. | |
| 16 | Quit Lymic and start it again with the toggle on. Without touching Settings, open an unhydrated placeholder. | It hydrates (the sync root is served again on app start). | |
| 17 | Switch the toggle off. Open an unhydrated placeholder, and a hydrated one. | The unhydrated one fails to open (record the Explorer error). The hydrated one still opens. The registration and placeholders stay (cleanup is #283). | |
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
| R16 | With the delete veto of #279 merged: remove a photo in Immich and refresh. | The removal still works, because the veto acknowledges deletes for which `adapter::reconcile::is_own_removal(path)` is true. If the delete is refused, the callback of #279 does not consult it. | |
| R17 | Change the Virtual Library folder (disable, choose another folder, enable) and open a directory. | `virtual_library_populated_directories` holds only directories of the new folder. | |

Things to look for beyond the table: whether `CfOpenFileWithOplock` works on a directory placeholder to read its identity (a directory that cannot be read counts as not ours and is left, which would keep a removed album's folder forever); whether reading the identity of thousands of placeholders per run is fast enough (a month with a few thousand photos); whether `CF_UPDATE_FLAG_DEHYDRATE` together with a new size works on a hydrated placeholder; whether `remove_dir` on a never-populated directory placeholder triggers `FETCH_PLACEHOLDERS` (it should not); whether Lymic's own deletion of a placeholder reaches `NOTIFY_DELETE` at all.

## Known gaps this record will not close

- Delete and rename of placeholders are not vetoed yet (#279). Do not test them as passing.
- Directories populated before reconciliation existed (#280) are not recorded as populated and are not reconciled; test reconciliation on a fresh enable.
- No in-app notice and no re-login offer on failure (#281); the platform error is all a person sees.
- Disabling does not unregister the sync root or remove files (#283).
- No thumbnails from Immich and no autostart hint yet; unhydrated placeholders work only while Lymic runs.
- Files whose size Immich does not report even after asking for the asset are left out of their directory (logged to standard error only).
