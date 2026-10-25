# Microsoft Store Public Release Record: Lymic

> Status: Entwurf. Die Checkliste in Abschnitt 4 wird erst nach Flight und Freigabe abgehakt.

This document records the official public release parameters for Lymic on Microsoft Store, documenting worldwide availability, category, localized listings, capability approval, and release-channel isolation.

---

## 1. Release Identification

| Attribute | Details |
|---|---|
| **Application** | Lymic |
| **Package Identity** | `xXRoxXeRXx.Lymic` |
| **Target Channel** | Microsoft Store (Windows 10/11 x64 MSIX) |
| **Release Version** | `0.21.0` (MSIX Package Version `0.21.0.0`) |
| **Availability** | Worldwide (All Microsoft Store markets) |
| **Pricing** | Free (No in-app purchases, subscriptions, or ads) |
| **Category** | Photo & video (Primary), Utilities & tools (Secondary) |
| **Content Rating** | All ages (IARC 3+ / USK 0) |

---

## 2. Release-Channel Isolation (ADR-0004)

Lymic maintains two isolated Windows distribution channels:
1. **GitHub / NSIS Installer Channel**:
   - Packaged as a standard NSIS installer executable.
   - Built with `LYMIC_RELEASE_CHANNEL=github`.
   - Uses Tauri's signed in-app updater against GitHub Releases.
2. **Microsoft Store MSIX Channel**:
   - Packaged as an x64 MSIX via Windows SDK `makeappx.exe`.
   - Built with `LYMIC_RELEASE_CHANNEL=store`.
   - In-app update checks, prompts, and downloads are completely omitted and disabled (`isUpdatesSupported === false`).
   - All application updates are managed exclusively by the Microsoft Store client.

---

## 3. Flight Evidence & Certification Links

- **Private Flight Validation**: Runbook und offene Kriterien in [`docs/store/flight-validation.md`](file:///c:/Users/meyer/Development/lymic/docs/store/flight-validation.md).
- **Restricted Capability Justification**: Declaring `runFullTrust` and `broadFileSystemAccess` for persistent user-selected Watched Folders and real-time filesystem change observation, documented in [`src-tauri/msix/justification.md`](file:///c:/Users/meyer/Development/lymic/src-tauri/msix/justification.md).
- **Store Listing & Metadata**: Complete German and English descriptions, categories, and screenshot assets documented in [`docs/store/listing.md`](file:///c:/Users/meyer/Development/lymic/docs/store/listing.md).
- **Public Legal & Help URLs**:
  - Privacy Policy: `https://xxroxxerxx.github.io/lymic/privacy/`
  - Support Page: `https://xxroxxerxx.github.io/lymic/support/`
- **Architecture Decision Record**: [ADR-0004: Microsoft Store and Release-Channel Isolation](file:///c:/Users/meyer/Development/lymic/docs/adr/0004-microsoft-store-release-channel.md).

---

## 4. Release Checklist & Verification Sign-Off

- [ ] Exact Flight-validated package (`Lymic_0.21.0_x64.msix`) submitted for worldwide public distribution.
- [ ] Store listing configured as Free, Photo & Video, with full German and English metadata.
- [ ] All-ages content questionnaire completed truthfully.
- [ ] Store-only updater channel verified; no conflicting GitHub updater UI.
- [ ] Restricted capability justification von Microsoft Partner Center genehmigt (ausstehend).
- [ ] GitHub Pages privacy and support pages live and HTTPS enforced.


