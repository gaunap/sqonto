# Sqonto – Buchhaltung

Lokale Buchhaltungssoftware für Selbstständige, Freiberufler und kleine Unternehmen. Läuft
vollständig auf dem eigenen Rechner (keine Cloud), die Daten liegen in einer SQLite-Datenbank.

## Funktionen

- **Verkauf:** Angebote, Auftragsbestätigungen, Lieferscheine, Rechnungen, Abschlags- und
  Schlussrechnungen, Gutschriften und Stornorechnungen – inklusive Umwandlung entlang des
  Vorgangs (Angebot → Auftrag → Lieferschein/Rechnung).
- **E-Rechnungen:** Erzeugen von ZUGFeRD (PDF/A-3 mit eingebettetem XML) und XRechnung 3.0
  (CII, EN 16931), Einlesen von ZUGFeRD/XRechnung bei der Belegerfassung.
- **Layouts:** Layout-Designer mit frei positionierbaren Elementen, Logo, Briefpapier (PDF/Bild),
  Schriften und Farben. PDF-Satz mit [Typst](https://typst.app).
- **Offene Posten & Mahnwesen:** Altersstruktur, Zahlungserinnerungen und Mahnstufen mit
  Gebühren und Verzugszinsen, Mahnlauf, GiroCode auf Rechnungen und Mahnungen.
- **Einkauf:** Belegerfassung (Upload, Kategorisierung, Aufteilung nach Steuersätzen, § 13b),
  Lieferanten automatisch aus E-Rechnungen anlegen.
- **Bank & Kasse:** CSV- und CAMT-Import mit automatischer Zuordnung, Zahlungen, Skonto,
  Ausbuchungen, GoBD-konformes Kassenbuch mit Stornobuchungen.
- **Buchhaltung:** Einnahmen-Überschuss-Rechnung (Ist-Versteuerung) oder doppelte Buchführung
  (Bilanzierung, Soll-Versteuerung), SKR03/SKR04, Buchungsjournal, Kontenblätter, manuelle
  Buchungen, Anlagenverzeichnis mit AfA (linear, GWG, Sammelposten), Festschreibung.
- **Steuern & Berichte:** Umsatzsteuer-Voranmeldung (inkl. Dauerfristverlängerung und
  Kleinunternehmerregelung), EÜR, GuV, Bilanz, BWA, Summen- und Saldenliste, Auswertungen,
  Dashboard, DATEV-Export (EXTF-Buchungsstapel), Beleg-ZIP, alle Berichte als PDF.

## Schnellstart (Windows)

Voraussetzungen: [Rust](https://rustup.rs) (stable) und [Node.js](https://nodejs.org) ab Version 22.

```bat
build.cmd
```

baut Frontend und Backend und legt die fertigen Programme unter `dist\` ab:

- **`Sqonto.exe`** – die Desktop-Anwendung. Ein Doppelklick (oder `start.cmd`) öffnet Sqonto in
  einem eigenen Fenster – ohne Konsole und ohne Browser. Ein erneuter Start holt das bereits
  geöffnete Fenster in den Vordergrund. PDFs zum Drucken öffnen sich in einem eigenen Fenster,
  Downloads fragen mit dem Dialog „Speichern unter“ nach dem Ziel. Beim Schließen mit
  ungespeicherten Änderungen fragt Sqonto nach.
- **`sqonto-server.exe`** – derselbe Server ohne Fenster, z. B. für die Nutzung im Browser
  (<http://localhost:8787>) oder für die Entwicklung.

Die Daten liegen im Ordner `dist\data` neben den Programmen (beide nutzen dieselben Daten). Zum
Umziehen oder Sichern genügt es, diesen Ordner zu kopieren – oder unter
*Einstellungen → Daten & Sicherung* eine Sicherung herunterzuladen.

Die Desktop-Anwendung nutzt die „Microsoft Edge WebView2 Runtime“, die unter Windows 10 und 11
vorinstalliert ist. Unter Linux/macOS entsprechend `./build.sh`; Linux benötigt dafür
`webkit2gtk-4.1`, mit `SQONTO_SERVER_ONLY=1 ./build.sh` wird nur der Server gebaut.

Beim ersten Start führt ein Assistent durch die Einrichtung. Mit *„Erst einmal ausprobieren“*
wird ein vollständiges Demo-Geschäftsjahr geladen.

## Konfiguration

| Umgebungsvariable    | Bedeutung                                                                | Standard                       |
| -------------------- | ------------------------------------------------------------------------ | ------------------------------ |
| `SQONTO_DATA_DIR`    | Datenverzeichnis (Datenbank, Dateien, Protokoll `sqonto.log`)            | `data` neben der Programmdatei |
| `SQONTO_PORT`        | Port des lokalen Servers (ist er belegt, wird der nächste freie genutzt) | `8787`                         |
| `SQONTO_DESKTOP_URL` | Desktop-Fenster zeigt diese Adresse statt des eingebauten Frontends      | –                              |
| `SQONTO_DEVTOOLS`    | gesetzt: Entwicklerwerkzeuge im Desktop-Fenster                          | nur in Debug-Builds            |

`sqonto-server --open` öffnet nach dem Start zusätzlich den Browser.

Der Server lauscht ausschließlich auf `127.0.0.1` und ist damit nur vom eigenen Rechner aus
erreichbar – auch wenn er in der Desktop-Anwendung läuft.

## Entwicklung

Backend und Frontend lassen sich weiterhin getrennt starten:

```bash
# Backend (Port 8787), Daten im Arbeitsverzeichnis unter ./data
cd backend && cargo run

# Frontend mit Hot Reload (Port 5173, leitet /api an das Backend weiter)
cd frontend && npm install && npm run dev
```

Die Oberfläche dann im Browser unter <http://localhost:5173> öffnen – oder im Desktop-Fenster:

```bash
cd backend && SQONTO_DESKTOP_URL=http://localhost:5173 cargo run --bin sqonto-desktop
```

Die TypeScript-Typen der API werden aus den Rust-Strukturen erzeugt (`ts-rs`):

```bash
cd frontend && npm run types
```

Tests: `cd backend && cargo test`, Typprüfung: `cd frontend && npm run typecheck`.

### Aufbau

- `backend/` – Rust-Bibliothek `sqonto` (Axum, SQLite über rusqlite, rust_decimal) mit zwei
  Programmen: `src/main.rs` (`sqonto-server`) und `src/bin/sqonto-desktop.rs` (Fenster mit
  WebView2/WebKit über `wry`/`tao`). Buchungslogik in `src/services/posting.rs`, Berichte in
  `src/services/reports.rs`, E-Rechnung in `src/services/einvoice.rs`, PDF-Erzeugung mit
  eingebettetem Typst in `src/pdf/` und `templates/`.
- `frontend/` – React, TypeScript, Mantine, TanStack Query. Das fertige Frontend wird beim
  Release-Build in die Programme eingebettet; `src/lib/desktop.tsx` verbindet es mit dem
  Desktop-Fenster (ungespeicherte Änderungen beim Schließen, Download-Hinweise).

## Lizenz

Copyright © 2026 die Sqonto-Mitwirkenden

Sqonto ist freie Software unter der [GNU Affero General Public License, Version 3 oder
später](LICENSE) (AGPL-3.0-or-later). Sie dürfen Sqonto nutzen, untersuchen, verändern und
weitergeben – auch kommerziell. Wer Sqonto weitergibt oder eine veränderte Fassung als Dienst
über ein Netzwerk anbietet, muss den Quellcode dieser Fassung unter derselben Lizenz zugänglich
machen.

Mit Sqonto erstellte Rechnungen, Berichte und sonstige Dokumente gehören Ihnen und unterliegen
nicht der AGPL.

Sqonto enthält Bibliotheken und Schriften anderer Projekte unter deren eigenen freien Lizenzen
(u. a. MIT, Apache-2.0, BSD, SIL Open Font License). Die Lizenzen der Schriften liegen in
[`backend/assets/fonts/LICENSE-FONTS.txt`](backend/assets/fonts/LICENSE-FONTS.txt); die
vollständige Übersicht aller Komponenten erzeugt der Build als `dist/THIRD-PARTY-LICENSES.txt`
(`node scripts/third-party-licenses.mjs`).

## Hinweis

Die Software unterstützt bei der Buchführung, ersetzt aber keine steuerliche Beratung und wird
ohne Gewährleistung bereitgestellt. Die Übermittlung der Umsatzsteuer-Voranmeldung erfolgt über
ELSTER; Sqonto liefert die Kennzahlen.
