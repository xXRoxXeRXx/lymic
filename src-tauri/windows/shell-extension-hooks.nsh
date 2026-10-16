; Explorer loads the provider in-process, so both COM activation and the
; Cloud Files sync-root association must point to the DLL in $INSTDIR.
!define LYMIC_PROVIDER_ID "{6B3AF5BC-6E32-40D8-B0A2-2C14B3FA481C}"
!define LYMIC_THUMBNAIL_CLSID "{D8D7474E-6E04-4534-8EF5-C4A90F01B395}"
!define LYMIC_SYNC_ROOT_KEY "SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager\${LYMIC_PROVIDER_ID}"
!define LYMIC_CLASS_KEY "Software\Classes\CLSID\${LYMIC_THUMBNAIL_CLSID}"
!define LYMIC_HOOK_DIR "${__FILEDIR__}"

!macro RegisterLymicThumbnailProvider HIVE
  WriteRegStr ${HIVE} "${LYMIC_CLASS_KEY}" "" "Lymic Thumbnail Provider"
  WriteRegStr ${HIVE} "${LYMIC_CLASS_KEY}\InprocServer32" "" "$INSTDIR\resources\lymic-shell-ext.dll"
  WriteRegStr ${HIVE} "${LYMIC_CLASS_KEY}\InprocServer32" "ThreadingModel" "Apartment"
  WriteRegStr ${HIVE} "${LYMIC_SYNC_ROOT_KEY}" "ThumbnailProvider" "${LYMIC_THUMBNAIL_CLSID}"
!macroend

!macro UnregisterLymicThumbnailProvider HIVE
  DeleteRegValue ${HIVE} "${LYMIC_SYNC_ROOT_KEY}" "ThumbnailProvider"
  DeleteRegKey ${HIVE} "${LYMIC_CLASS_KEY}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; `both` is elevated, so the installer can support all users as well as a
  ; per-user installation. Runtime setup repeats HKCU when the library is on.
  CreateDirectory "$INSTDIR\resources"
  SetOutPath "$INSTDIR\resources"
  File /oname=lymic-shell-ext.dll "${LYMIC_HOOK_DIR}\..\..\lymic-shell-ext\build\Release\lymic-shell-ext.dll"
  !insertmacro RegisterLymicThumbnailProvider HKLM
  !insertmacro RegisterLymicThumbnailProvider HKCU
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro UnregisterLymicThumbnailProvider HKLM
  !insertmacro UnregisterLymicThumbnailProvider HKCU
!macroend
