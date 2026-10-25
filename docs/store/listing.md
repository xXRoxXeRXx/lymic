# Microsoft Store Listing: Lymic

This document contains the official metadata, localized descriptions, questionnaire answers, and asset specifications for submitting Lymic to Microsoft Partner Center.

---

## 1. Product Identification & Categorization

| Field | Value |
|---|---|
| **App Name** | Lymic |
| **Package Identity** | `xXRoxXeRXx.Lymic` |
| **Publisher** | `CN=EB6D92C3-1B82-4D11-B562-6EA2D7C2FDEC` |
| **Publisher Display Name** | `xXRoxXeRXx` |
| **Package Family Name (PFN)** | `xXRoxXeRXx.Lymic_5pq7hxkq5cr6j` |
| **Package SID** | `S-1-15-2-2281093430-885897720-1483119853-4043636694-2962100176-2089881768-2617100744` |
| **Store ID** | `9NLQWHTHR1FZ` |
| **Store URL** | `https://apps.microsoft.com/detail/9NLQWHTHR1FZ` |
| **Store Protocol Link** | `ms-windows-store://pdp/?productid=9NLQWHTHR1FZ` |
| **MSA-App-ID** | `b2c23d01-1c1f-4143-931d-221a2a54717b` |
| **Primary Category** | Photo & video |
| **Secondary Category** | Utilities & tools |
| **Pricing** | Free (Worldwide, all markets) |
| **Distribution Channel** | Microsoft Store (x64 MSIX) |

---

## 2. Public Privacy & Support URLs

| Document | Public URL | Notes |
|---|---|---|
| **Privacy Policy** | `https://xxroxxerxx.github.io/lymic/privacy/` | Hosted on GitHub Pages; declares zero ads, zero telemetry, local storage only, and direct transfer to user Immich server. |
| **Support Page** | `https://xxroxxerxx.github.io/lymic/support/` | Hosted on GitHub Pages; includes troubleshooting, issue reporting, and maintainer contact. |

---

## 3. Localized Store Listings

### English (en-US)

#### Short Description / Tagline (100 characters max)
> High-performance desktop synchronization client for self-hosted Immich media servers.

#### Full Description
```text
Lymic is a fast, lightweight desktop synchronization client built specifically for self-hosted Immich media servers. It automatically discovers, monitors, and uploads photos and videos from your local folders and removable storage volumes directly to your personal Immich instance.

Key Features:
- Watched Folders: Configure multiple local directories and external drives to continuously monitor for new or updated media assets.
- Removable Media Detection: Automatically detects connected SD cards and external drives, synchronizing media seamlessly when plugged in.
- Live Photo Support: Reliably pairs and uploads still photos and companion video clips together as unified Live Photos.
- Flexible Album Assignment: Map specific folders to existing Immich albums or create new albums on the fly.
- Granular Exclusion Patterns: Exclude temporary files, raw camera caches, or specific extensions using intuitive relative patterns.
- High-Performance Sync Engine: Concurrent, resilient uploads with automatic retries, deduplication, and low memory usage.
- Native Credential Security: Stores authentication tokens and API keys safely in your operating system's native credential vault.
- Complete Privacy: No ads, no third-party telemetry, and no tracking. All data flows directly and exclusively between your computer and your configured Immich server.

Important Note:
Lymic is an independent open-source project and is not affiliated with, endorsed by, or officially supported by the Immich project. An accessible, running Immich server instance is required to use this application.
```

#### Feature Highlights (Bulleted)
- Continuous background monitoring of user-selected Watched Folders
- Automatic removable volume detection for SD cards and external drives
- Native Live Photo pairing and target album management
- Customizable file and directory exclusion patterns
- Secure OS keyring credential storage
- Zero telemetry, analytics, or tracking

---

### Deutsch (de-DE)

#### Kurzbeschreibung / Untertitel (max. 100 Zeichen)
> Schneller Desktop-Synchronisations-Client für selbst gehostete Immich-Medienserver.

#### Ausführliche Beschreibung
```text
Lymic ist ein schneller und ressourcenschonender Desktop-Client für selbst gehostete Immich-Medienserver. Die Anwendung überwacht ausgewählte lokale Ordner sowie Wechselspeichermedien und synchronisiert Fotos und Videos direkt mit Ihrer persönlichen Immich-Instanz.

Wichtigste Funktionen:
- Überwachte Ordner: Konfigurieren Sie mehrere Verzeichnisse auf internen oder externen Laufwerken für eine kontinuierliche automatische Erkennung neuer Medien.
- Wechsellaufwerks-Erkennung: Erkennt angeschlossene Speicherkarten und externe Festplatten automatisch und nimmt die Synchronisation bei Verbindung nahtlos auf.
- Live-Photo-Unterstützung: Erkennt Bild- und Videokomponenten von Live-Photos und lädt sie zusammengehörig hoch.
- Zielalben-Verwaltung: Weisen Sie überwachten Ordnern bestehende Immich-Alben zu oder erstellen Sie neue Alben direkt aus der App.
- Ausschlussregeln: Schließen Sie temporäre Dateien, Exporte oder bestimmte Dateiendungen über relative Muster gezielt von der Synchronisation aus.
- Leistungsstarke Synchronisation: Parallele, fehlertolerante Uploads mit Prüfsummen-Abgleich und automatischer Wiederaufnahme bei Verbindungsunterbrechungen.
- Sichere Anmeldung: Zugangsdaten und API-Schlüssel werden ausschließlich im nativen Windows-Anmeldeinformationsspeicher verwahrt.
- Volle Privatsphäre: Keine Werbung, kein Tracking und keine Telemetrie. Alle Datenübertragungen erfolgen ausschließlich zwischen Ihrem PC und Ihrem eigenen Server.

Hinweis:
Lymic ist ein unabhängiges Open-Source-Projekt und steht in keiner offiziellen Verbindung zum Immich-Projekt. Für die Nutzung wird ein lauffähiger, erreichbarer Immich-Server benötigt.
```

#### Funktions-Highlights (Stichpunkte)
- Automatische Hintergrundüberwachung ausgewählter lokaler Ordner
- Erkennung von SD-Karten und Wechsellaufwerken beim Anschließen
- Zusammenführung von Live Photos und Zuweisung zu Immich-Alben
- Einstellbare Ausschlussregeln für Dateimuster und Verzeichnisse
- Schutz von Zugangsdaten im Windows-Schlüsselbund
- Keine Telemetrie, Analyse oder Werbung

---

## 4. Product Screenshots & Captions

All screenshots are stored in `docs/store/assets/` (minimum resolution 1366x768 / 1920x1080, 16:9).

| File | Feature | English Caption | Deutsche Bildunterschrift |
|---|---|---|---|
| `docs/store/assets/01_signin.png` | Sign-in | Connect directly to your self-hosted Immich server with credentials or API key | Direkte Verbindung zum eigenen Immich-Server per Zugangsdaten oder API-Schlüssel |
| `docs/store/assets/02_watched_folders.png` | Watched Folders | Manage watched folders, removable media, and custom exclusion rules | Verwaltung überwachter Ordner, Wechsellaufwerke und Ausschlussregeln |
| `docs/store/assets/03_synchronization.png` | Synchronization | Real-time synchronization progress, transfer speeds, and queue monitoring | Echtzeit-Fortschrittsanzeige, Transferrate und Warteschlangen-Status |
| `docs/store/assets/04_settings.png` | Settings | Configure system autostart, parallel upload concurrency, and dark/light themes | Einstellungen für Autostart, parallele Uploads und Erscheinungsbild |

---

## 5. Age Rating & Content Questionnaire (IARC / All-Ages)

| Question Category | Response | Justification |
|---|---|---|
| **Violence** | No | The application contains no depictions of violence. |
| **Sexuality & Nudity** | No | The application displays no sexual or adult content. |
| **Profanity / Crude Humor** | No | No crude language or humor is included. |
| **Controlled Substances** | No | No references to drugs, alcohol, or tobacco. |
| **Gambling** | No | No simulated or real-money gambling. |
| **User-to-User Communication** | No | Lymic is a single-user synchronization client; there is no public chat, messaging, or forum. |
| **User Location Sharing** | No | Physical location is never captured or broadcast. |
| **In-App Purchases / Commerce** | No | The app is completely free with no monetized features. |
| **Target Audience** | All ages (General audience) | Suitable for users of all age brackets. |

---

## 6. Restricted Capabilities Justification

Lymic declares `runFullTrust` and `broadFileSystemAccess` in `AppxManifest.xml`.
The complete Partner Center certification submission text is maintained in:
[`src-tauri/msix/justification.md`](file:///c:/Users/meyer/Development/lymic/src-tauri/msix/justification.md)

Key justification summary:
1. Persistent folder watching across system reboots for user-selected internal and external directories without repetitive picker interactions.
2. Low-level directory notification observation (`ReadDirectoryChangesW`) to automatically detect media file creation and modifications in real time.
3. Removable volume reconciliation when camera SD cards or backup drives are reconnected.
