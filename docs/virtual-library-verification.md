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
| 28 | **Hydration failures (#281), NOT YET RUN.** With the Virtual Library on and Lymic open, stop (or block) the Immich server. Open an unhydrated placeholder in Photos or Paint, and time it. | The open fails within about 15 seconds (connect and response timeout), with a network-style error ("network not connected" or the cloud provider "not available" message); Explorer does not hang. Immich's access log (or a proxy) shows exactly one attempt for the open: no retry loop. The shell status code is `STATUS_CLOUD_FILE_NETWORK_UNAVAILABLE`. | |
| 29 | Same situation: Lymic window open, on any view. | Exactly one notice appears (a banner above the content, and one activity log entry: "Could not open ... because Immich could not be reached."). It has no "Sign in again" button. | |
| 30 | Still offline: open ten different unhydrated placeholders (or select ten and press Enter), then open the same one again several times. | Every open fails promptly, but no second notice and no second log entry appear. | |
| 31 | Dismiss the banner, restore the server, open a placeholder (it hydrates), stop the server again and open another one. | A new notice appears: a success ended the episode. Also, with the server offline for longer than five minutes, one more notice appears at the next failure after the five quiet minutes. | |
| 32 | Restore the server. Revoke the API key in Immich (or log in with a revoked key), then open an unhydrated placeholder. | The open fails promptly with an authentication error (`STATUS_CLOUD_FILE_AUTHENTICATION_FAILED`), no retry loop. One notice appears: "... your Immich sign-in is no longer valid", with a "Sign in again" button. Record the exact Explorer or app error text. | |
| 33 | Press "Sign in again". | Lymic shows the login form for the same server with the explanation and a "Not now" button. Nothing was logged out: the Virtual Library stays enabled and its folder is untouched. Enter a new valid API key: the form closes, Lymic is signed in, and opening the same placeholder now hydrates. Repeat once and press "Not now": the form closes and nothing changes. | |
| 34 | Delete (or move to the Immich trash and empty it) a photo in the Immich web app, without refreshing the Virtual Library. Open its (unhydrated) placeholder in Explorer. | The open fails promptly with a "file not found" style error (record the actual Explorer or app text). One notice appears naming the file, saying it was removed from Immich. **Open question to record:** whether the platform accepts `STATUS_OBJECT_NAME_NOT_FOUND` as the completion status (Explorer shows an error and the read ends) or substitutes another error; if it misbehaves, the one-line fallback is `STATUS_CLOUD_FILE_INVALID_REQUEST` in `ntstatus` in `adapter/callbacks.rs`. | |
| 35 | After 34, wait for the next reconciliation (#280, when it has landed) or press Refresh Library. | The placeholder is gone from the folder. (Until #280 lands this is expected to fail: record "not applicable".) | |
| 36 | Start opening a large video (more than 1 GiB) so that it downloads, then close the program that opened it (or cancel the copy in Explorer) after a few seconds. Watch Immich's access log and Lymic's network use. | The download stops within a moment (the connection to Immich closes). Lymic's standard error shows "a download was cancelled by the reader and stopped". No notice appears. If instead the note "a cancel arrived for a download that is not (or no longer) in flight" shows and the download continues, the cancel callback's transfer key does not match the fetch's: record it. | |
| 37 | Take Immich offline in the middle of a large download (the `FETCH_DATA` is running). | The read fails within about 30 seconds (no data for 30 seconds) with a network error; one notice. The partly hydrated file is not marked as complete. | |
| 38 | Switch Lymic's language to German and Arabic and trigger the notices of 29, 32 and 34 once each. | The banner, the log entry, "Sign in again", "Not now" and the explanation on the login form are in the chosen language and read correctly. | |
| 39 | Open a folder you have not browsed while the server is stopped, then again while it is still stopped. | The listing fails each time; the notice is the same episode as in 29 (not a second one). | |

## Known gaps this record will not close

- Delete and rename of placeholders are not vetoed yet (#279). Do not test them as passing.
- Placeholders are not refreshed after the first listing (#280); a changed Immich Library shows after the next listing only if the directory was never populated.
- Hydration failures (#281): the Windows error path (completing `FETCH_DATA` with a failure status, the `CANCEL_FETCH_DATA` callback) has no unit test and is covered only by steps 28 to 39 above, which have not been run. What is unit tested: the classification of Immich failures, the failure to Cloud Files status mapping, the timeouts, the episode rule for notices, the cancel registry and the notice and re-login wiring in the frontend.
- Disabling does not unregister the sync root or remove files (#283).
- No thumbnails from Immich and no autostart hint yet; unhydrated placeholders work only while Lymic runs.
- Files whose size Immich does not report even after asking for the asset are left out of their directory (logged to standard error only).
