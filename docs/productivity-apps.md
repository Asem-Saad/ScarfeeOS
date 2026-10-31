# Productivity apps

Research for lifecycle stage 6: which existing program ScarfeeOS should use for documents, spreadsheets, presentations, email, media and the browser. Policy: [ip-and-licensing.md](ip-and-licensing.md) rules 2 (dependencies), 4 (codecs, open ODF/OOXML), 5 (WebView browser first) and 6 (no trademarks). This page recommends; it does not approve anything for shipping, and stage 6 has not started.

Status wording follows `docs/models.md`: **verified** = read on the primary source (spec file, license file, tracker, source file) on 2026-10-31. **not verified** = not read, not reachable, or the sources disagree.

## How this was checked

- Date: 2026-10-31. Target: Fedora 44, Wayland, English plus Arabic with RTL, a 16 GB laptop shared with a local model.
- **Nothing was installed, started or run.** The research host was Ubuntu 24.04. So every statement about Wayland, Arabic rendering, driving and speed comes from specs, source, documentation or trackers, never from a test. **No memory was measured for any program**; no primary source with figures was found. Sizes below are package sizes, not RAM.
- Fedora package data: `https://mdapi.fedoraproject.org/f44/pkg/<package>` (per-RPM download and installed size, dependencies), `https://bodhi.fedoraproject.org/updates/?packages=<package>&releases=F44` (stable or testing) and `https://src.fedoraproject.org/rpms/<package>/raw/f44/f/<package>.spec` (RPM license tag, build flags). Sizes are per package and exclude dependencies already on the system. mdapi lags the other two: its repo label and version are stale for several packages, so Bodhi and the spec are used for status and version, and mdapi only for sizes.

  | Package | mdapi says | Bodhi or spec says |
  |---|---|---|
  | firefox | 150.0, updates-testing | 159.0, stable 2026-10-29 ([spec](https://src.fedoraproject.org/rpms/firefox/raw/f44/f/firefox.spec)) |
  | chromium | 146.0.7680.80 | 156.0.8078.73, stable 2026-10-31 |
  | falkon, calligra | 25.12.3 | 26.08.1 stable (Bodhi); calligra spec at 26.08.2 |
  | kmail | 25.12.3 | spec at 26.08.2; which version is stable was not checked |
  | abiword, mpv, showtime | updates-testing | stable ([abiword](https://bodhi.fedoraproject.org/updates/FEDORA-2026-429b98f748)) |

- Bug counts come from tracker search APIs (title or text match) and are **samples, not exhaustive counts**. A bug that never uses the words "Arabic", "RTL" or "bidi" is not counted. A low count can mean few Arabic users report there, not that the program is good.
- "Arabic and RTL" means what the trackers and release notes show; none of it was seen on screen.

## Summary

| Category | Recommendation | Main reason | Main cost |
|---|---|---|---|
| Documents | LibreOffice Writer | Only candidate with a scripting API (UNO), open ODF and OOXML, and an Arabic language pack; new code is MPL-2.0 | About 333 MB installed for Writer and Impress; many open RTL bugs (tables, DOCX) |
| Presentations | LibreOffice Impress | Only complete presentation editor with a driving API | Open bugs: new slides ignore an RTL locale, slideshow and transition bugs on Wayland |
| Spreadsheets | LibreOffice Calc | RTL sheets, Eastern Arabic digits and Hijri formats exist; XLSX and ODS read and write | About 345 MB with the core; open RTL cosmetics bugs |
| Email | Thunderbird as the human interface; the assistant does mail itself over IMAP and SMTP from the Rust core | MPL-2.0 option, native Wayland in Fedora's launcher, richest command line | 386 MB; open composer and text-direction bugs |
| Media | mpv, driven only over its JSON IPC socket | 4.8 MB, one process, documented IPC, native Wayland | GPL build, so never linked; open libass Arabic bidi bugs |
| Browser | The project's own Tauri shell on `webkit2gtk4.1` (rule 5); Epiphany (WebKitGTK 6.0) as the Fedora stand-in | Same WebKit engine; Epiphany is scriptable through WebKitWebDriver today | The shell must be built; WebKit has open Arabic shaping bugs; Epiphany is a GPL-3.0+ app and Fedora's package lags |

Three programs matter for the memory budget. LibreOffice is one `soffice.bin` per profile for all three office apps, so its cost is paid once. Thunderbird and the browser each bring a full web engine. Anything on KDE's Akonadi or Qt WebEngine adds roughly 290 MB or more and was ruled out for that reason.

## Documents and presentations

LibreOffice covers both; the other candidates cover less. They are kept in one section because Writer and Impress share a core.

| | LibreOffice Writer and Impress | AbiWord (documents only) | Calligra Words and Stage |
|---|---|---|---|
| F44 version | 26.2.6.3-1, stable 2026-09-09 ([Bodhi](https://bodhi.fedoraproject.org/updates/FEDORA-2026-4e3b934406)) | 3.0.8-2, stable 2026-03-15 | 26.08.1 stable (Bodhi) |
| Installed size | core 302.8 MB, writer 13.2, impress 0.8, ure 6.9, data 3.4, Arabic langpack 3.1, one GTK plugin 2.0: about 333 MB ([core](https://mdapi.fedoraproject.org/f44/pkg/libreoffice-core)) | 0.2 + libabiword 24.2 MB ([libabiword](https://mdapi.fedoraproject.org/f44/pkg/libabiword)) | about 36 MB, plus `qt6-qtwebengine` 290.8 MB if absent ([calligra-libs](https://mdapi.fedoraproject.org/f44/pkg/calligra-libs)) |
| License | MPL-2.0 plus Apache-2.0, LGPL-3.0, BSD, MIT in the [RPM tag](https://src.fedoraproject.org/rpms/libreoffice/raw/f44/f/libreoffice.spec) (line 97); source headers say [MPL-2.0](https://raw.githubusercontent.com/LibreOffice/core/master/TEMPLATE.SOURCECODE.HEADER) | GPL-2.0-or-later ([spec](https://src.fedoraproject.org/rpms/abiword/raw/f44/f/abiword.spec), [COPYING](https://gitlab.gnome.org/World/AbiWord/-/raw/master/COPYING)) | GPL and LGPL mix ([spec](https://src.fedoraproject.org/rpms/calligra/raw/f44/f/calligra.spec), line 15) |
| Rule 2 | Preferred license; run as a separate program anyway | GPL: separate program only | GPL in the tag: separate program only |
| Wayland | Native through the GTK3 plugin (KDE Frameworks 6 plugin on Plasma 6); source reading only | GTK3 app, no statement found | Qt6 app, no statement found |
| Driving | `soffice --headless --convert-to`, UNO over `--accept` socket with `libreoffice-pyuno` | `abiword --to=FORMAT`, an `AbiCommand` REPL plugin (not confirmed in the RPM) | `--export-pdf`, `--print`, D-Bus adaptors in source (not tested) |

### LibreOffice

- **License.** Upstream carries three license files: [`COPYING`](https://raw.githubusercontent.com/LibreOffice/core/master/COPYING) (GPL v3 text), [`COPYING.LGPL`](https://raw.githubusercontent.com/LibreOffice/core/master/COPYING.LGPL) and [`COPYING.MPL`](https://raw.githubusercontent.com/LibreOffice/core/master/COPYING.MPL). The GPL v3 file's scope is not explained in the files read (not verified). In the Fedora tag, GPL appears only inside a choice-of clause. Rule 4: it is an open ODF and OOXML implementation, with [DOCX, XLSX and PPTX load and save](https://raw.githubusercontent.com/LibreOffice/help/master/source/text/shared/guide/ms_import_export_limitations.xhp).
- **Java is not needed** for Writer, Calc or Impress: `libreoffice-core` only has `Recommends: java-headless`; the hard `java-25-headless` requirement is in `libreoffice-base` ([spec](https://src.fedoraproject.org/rpms/libreoffice/raw/f44/f/libreoffice.spec)). JDK 25 headless is 248.4 MB installed ([mdapi](https://mdapi.fedoraproject.org/f44/pkg/java-25-openjdk-headless)). Whether Fedora's weak dependencies pull it in on ScarfeeOS is an image decision.
- **Arabic and RTL: supported, with a long open list.**
  - Recent work in 26.2: automatic paragraph direction while typing (tdf#162120), RTL and complex-script editing features always shown (tdf#168719) ([26.2 release notes](https://wiki.documentfoundation.org/ReleaseNotes/26.2)).
  - Sample of open bugs matching Arabic, RTL, bidi, CTL or right-to-left in the title: Writer 85, Impress 22. Writer: [162302](https://bugs.documentfoundation.org/show_bug.cgi?id=162302) Arabic ligature causes crash or painting failures (UNCONFIRMED); [155470](https://bugs.documentfoundation.org/show_bug.cgi?id=155470) Eastern Arabic digits turn Latin when saved as DOCX (REOPENED); [173785](https://bugs.documentfoundation.org/show_bug.cgi?id=173785) image jumps cell in RTL tables (NEW); [155489](https://bugs.documentfoundation.org/show_bug.cgi?id=155489) DOCX RTL meta bug (NEW); [166221](https://bugs.documentfoundation.org/show_bug.cgi?id=166221) lag in mixed RTL and LTR text (UNCONFIRMED).
  - Impress: [145729](https://bugs.documentfoundation.org/show_bug.cgi?id=145729) an RTL locale does not set the default direction of new documents (NEW) and [145730](https://bugs.documentfoundation.org/show_bug.cgi?id=145730) templates are not flipped for RTL locales (NEW); both matter for an OS that defaults to RTL. [139905](https://bugs.documentfoundation.org/show_bug.cgi?id=139905) PPTX RTL meta bug (NEW).
  - Cross-product tracker: [112810](https://bugs.documentfoundation.org/show_bug.cgi?id=112810) "Arabic and Farsi language-specific issues" (NEW, 94 dependencies, mixed Writer, Calc and others).
  - The F44 build is 26.2; later RTL improvements in 26.8 are not in it ([26.8 notes](https://wiki.documentfoundation.org/ReleaseNotes/26.8)).
  - Not verified: Arabic font coverage of the default install; Eastern Arabic digits in Writer beyond the bug above.
- **Memory and disk.** About 333 MB installed for Writer and Impress; 123.4 MB download for the core. Upstream's stated Linux minimum is "256 MB RAM (512 MB RAM recommended)" ([system requirements](https://www.libreoffice.org/get-help/system-requirements/)); that is a minimum, not a measurement. A second `soffice` launch finds the first through an IPC pipe and hands over its arguments ([source](https://raw.githubusercontent.com/LibreOffice/core/master/desktop/source/app/officeipcthread.cxx)), so all documents of one profile share one process. Whether the JVM starts during Writer or Impress use was not checked.
- **Wayland.** The RPM ships four plugins: x11, gtk3, gtk4 ("experimental") and kf6. Autodetect picks `kf6` on Plasma 6, then `gtk3`, then X11; `gtk4` is only reachable by `SAL_USE_VCLPLUGIN=gtk4` ([salplug.cxx](https://raw.githubusercontent.com/LibreOffice/core/master/vcl/source/app/salplug.cxx), [desktopdetector.cxx](https://raw.githubusercontent.com/LibreOffice/core/master/vcl/unx/generic/desktopdetect/desktopdetector.cxx)). Open Wayland bugs relevant here: [173050](https://bugs.documentfoundation.org/show_bug.cgi?id=173050) most slide transitions broken on KF6 and Qt6 on Wayland (NEEDINFO); [172030](https://bugs.documentfoundation.org/show_bug.cgi?id=172030) slideshow opens a separate window (UNCONFIRMED); [173491](https://bugs.documentfoundation.org/show_bug.cgi?id=173491) Calc cursor overlay not painted with gtk3 on Wayland; [146153](https://bugs.documentfoundation.org/show_bug.cgi?id=146153) building without X11 is not possible yet (NEW, so X11 libraries stay a dependency). Meta bug: [100156](https://bugs.documentfoundation.org/show_bug.cgi?id=100156). Set the plugin explicitly in the image rather than relying on autodetect.
- **Driving.** Flags (`--headless`, `--convert-to`, `--show`, `--print-to-file`, `--accept`, `-env:UserInstallation=`) are in the [start parameters help](https://help.libreoffice.org/latest/en-US/text/shared/guide/start_parameters.html); that page is the "latest" help (26.8), so a flag may be newer than 26.2. The scripting route is UNO: `soffice --headless --accept="socket,host=localhost,port=2002;urp;"` and Python `import uno` from `libreoffice-pyuno` ([Developer's Guide](https://wiki.documentfoundation.org/Documentation/DevGuide/Professional_UNO), [example](https://raw.githubusercontent.com/LibreOffice/core/master/odk/examples/python/DocumentHandling/DocumentLoader.py)). Two caveats: `--accept` is ignored when an instance is already running, and a concurrent `--convert-to` is routed to that instance unless it has its own `-env:UserInstallation` profile (from the relay code; not run). **There is no D-Bus control interface on an RPM install**: the D-Bus path exists only when `LIBO_FLATPAK` is set ([officeipcthread.cxx](https://raw.githubusercontent.com/LibreOffice/core/master/desktop/source/app/officeipcthread.cxx), line 683). Bind the UNO socket to localhost or, better, use a Unix pipe accept string; any local process can use an open TCP socket.
- **Not verified:** all runtime behaviour; RAM of an interactive session; whether `.uno:SheetRightToLeft` is the exact dispatch string; Hijri and digit behaviour in Writer.

### AbiWord

- Light and simple (`abiword --to=rtf file.abw` converts, [man page](https://gitlab.gnome.org/World/AbiWord/-/raw/master/abiword.1)), but documents only. License GPL-2.0-or-later, so a separate program only; never link `libabiword`.
- Dependencies are aging: `libsoup-2.4`, `loudmouth`, `telepathy-glib` in `libabiword`'s requires ([mdapi](https://mdapi.fedoraproject.org/f44/pkg/libabiword)). The GTK4 port is only an open issue ([#31](https://gitlab.gnome.org/World/AbiWord/-/work_items/31)).
- Arabic: its own README calls the shaping engine "very basic" ([BiDiReadme.txt](https://gitlab.gnome.org/World/AbiWord/-/raw/master/BiDiReadme.txt)), but it is old text and the build links HarfBuzz and Pango, so what the code does now is not verified. The only RTL issue among its 23 open ones is [#26](https://gitlab.gnome.org/World/AbiWord/-/work_items/26) (RTL alignment, no comments).
- Not verified: Arabic quality, Wayland, format fidelity, runtime memory.
- Rejected: no presentations, small ecosystem, no confidence for fully RTL use.

### Calligra Words and Stage

- Real packages; they need `libQt6WebEngineCore` (`calligra-libs` requires), about 291 MB installed unless the browser already ships it. One process per document (`KDBusService::Multiple`, [KoApplication.cpp](https://invent.kde.org/office/calligra/-/raw/master/libs/main/KoApplication.cpp)). No scripting API like UNO; command line has `--export-pdf` and `--print`; D-Bus adaptors exist in source but were not tested.
- Arabic: three CONFIRMED Words bugs from 2011, [278179](https://bugs.kde.org/show_bug.cgi?id=278179), [285496](https://bugs.kde.org/show_bug.cgi?id=285496) and [286971](https://bugs.kde.org/show_bug.cgi?id=286971); whether they are fixed in the Qt6 port was not tested. Stage: 126 open bugs, none matched.
- Stage imports PPTX but the source tree has no PPTX export filter ([filters/stage/pptx](https://invent.kde.org/api/v4/projects/office%2Fcalligra/repository/tree?path=filters/stage/pptx)).
- Rejected: WebEngine dependency, GPL-heavy tag, no automation API, no Wayland statement.

### Helpers, not editors

- `pandoc-cli` 3.7.0.2 (214.1 MB installed, GPL-2.0-or-later: [spec](https://src.fedoraproject.org/rpms/pandoc-cli/raw/f44/f/pandoc-cli.spec), [COPYING.md](https://raw.githubusercontent.com/jgm/pandoc/main/COPYING.md)) turns Markdown into DOCX, ODT or PPTX with no GUI and suits an assistant that generates documents, run as a separate program. Open RTL gaps exist, for example [#7695](https://github.com/jgm/pandoc/issues/7695) DOCX writer does not set tables RTL. Not a replacement for an editor.
- `pdfpc` (GPL-3.0-or-later, [spec](https://src.fedoraproject.org/rpms/pdfpc/raw/f44/f/pdfpc.spec)) is a PDF presenter console that needs the WebKitGTK stack; Arabic not assessed.

### Choice

LibreOffice Writer and Impress. If memory or disk must come down, the first lever is leaving out Java, Calc and Base, not switching editors. Set the document default direction in the image because of bugs 145729 and 145730.

## Spreadsheets

| | LibreOffice Calc | Gnumeric | Calligra Sheets |
|---|---|---|---|
| F44 version | 26.2.6.3, stable | 1.12.59-2, stable ([Bodhi](https://bodhi.fedoraproject.org/updates/?search=gnumeric&releases=F44)) | 26.08.x (versions disagree, see top) |
| Installed size | calc 28.8 MB on top of the core (about 345 MB in all) | gnumeric 38.3 + goffice 8.7 + libgsf 1.0 MB, about 48 MB, plus GTK3 | about 70 MB plus `qt6-qtwebengine` 290.8 MB |
| License | as Writer above | GPL-2.0-only AND GPL-3.0-only AND LGPL-2.1-or-later ([spec](https://src.fedoraproject.org/rpms/gnumeric/raw/f44/f/gnumeric.spec)); upstream offers GPL 2 or 3 ([COPYING](https://gitlab.gnome.org/GNOME/gnumeric/-/raw/master/COPYING)) | GPL and LGPL mix ([spec](https://src.fedoraproject.org/rpms/calligra/raw/f44/f/calligra.spec)); README: mostly [LGPL 2+, also GPL](https://invent.kde.org/office/calligra/-/raw/master/README.md) |
| RTL sheet | Yes, menu and UNO property `TableLayout` | Yes, Format > Sheet > Direction; saved in `.gnumeric`, ODS and XLSX | Honoured when loading ODF; no toggle found ([465116](https://bugs.kde.org/show_bug.cgi?id=465116) open) |
| Eastern Arabic digits | Yes, `[NatNum1]` | No: [#779](https://gitlab.gnome.org/GNOME/gnumeric/-/issues/779) open, maintainer: "We do not have the expertise to do this right." | Not found |
| Hijri dates | Yes, `[~hijri]` | No plugin (only Hebrew and "christian" date plugins) | Not found |
| XLSX | Read and write | Read and write | Read only |
| Driving | `--convert-to`, UNO | `ssconvert` (no display needed per man page), no remote control | `calligraconverter`, D-Bus adaptors in source |

### LibreOffice Calc

- License as above. The Arabic features are documented in the [number format help](https://raw.githubusercontent.com/LibreOffice/help/master/source/text/shared/01/05020301.xhp): `[NatNum1]` for Eastern Arabic digits and `[~hijri]` for the Hijri calendar. Sheet direction is the UNO property `TableLayout` with `WritingMode2::RL_TB` ([cellsuno.cxx](https://raw.githubusercontent.com/LibreOffice/core/master/sc/source/ui/unoobj/cellsuno.cxx)).
- Open bugs, 15 in Calc matching RTL in the title (sample), mostly interface mirroring: [63444](https://bugs.documentfoundation.org/show_bug.cgi?id=63444) dialogs and lists in RTL sheets expand outside the window; [172252](https://bugs.documentfoundation.org/show_bug.cgi?id=172252) formula bar ignores the cell's RTL direction; [172251](https://bugs.documentfoundation.org/show_bug.cgi?id=172251) inequality signs are not reversed in RTL cells; [174122](https://bugs.documentfoundation.org/show_bug.cgi?id=174122) saving XLSX with Arabic text rewrites the Latin font to Tahoma (reported on 26.8.0.3, UNCONFIRMED). Calc typing and direction improvements in 26.8 are not in F44.
- Not verified: that Arabic UI and RTL render correctly in 26.2.6.3 on Wayland; the digit and Hijri behaviour at runtime.

### Gnumeric

- GPL only, and its `libspreadsheet` and goffice libraries are GPL too ([goffice COPYING](https://gitlab.gnome.org/GNOME/goffice/-/raw/master/COPYING): "the licenses are GPL, not LGPL"), so ScarfeeOS code must never link them. As a separate program it is allowed.
- Small, GTK3, and `ssconvert --set CELL=CONTENTS` plus `-T`/`-O` make it the easiest to drive from a script ([man page](https://gitlab.gnome.org/GNOME/gnumeric/-/raw/master/doc/ssconvert.1)). RTL works, but without Eastern Arabic digits or Hijri it is weaker for Arabic users. One dominant maintainer: 149 of the last 200 commits since 2026-01-01 are by one person. Fedora's 1.12.59 is behind upstream.
- Role: possible lightweight headless helper for table conversion. Not the main spreadsheet.

### Calligra Sheets, VisiData, sc-im

- Calligra Sheets: WebEngine dependency, no found RTL toggle, no XLSX export, unclear Fedora version. Rejected.
- VisiData (`GPL-3.0-only`) and sc-im (`BSD-4-Clause`: its advertising clause is outside the preferred list, [LICENSE](https://github.com/andmarti1424/sc-im/blob/main/LICENSE)) are terminal tools, not a spreadsheet interface for Arabic users. Bidi in a terminal depends on the terminal emulator, which was not checked.

### Choice

LibreOffice Calc. Gnumeric's `ssconvert` is an optional lighter helper for headless conversion only.

## Email

Whichever program is chosen, **the assistant should read and send mail itself over IMAP and SMTP from the Rust core**. None of the graphical clients has a mail API: Thunderbird's `-compose` only opens a compose window and the user must press Send (no silent-send flag found in [MessengerContentHandler.sys.mjs](https://hg.mozilla.org/comm-central/raw-file/tip/mail/components/MessengerContentHandler.sys.mjs)); Evolution's D-Bus services cover calendar and contacts, not mail; KMail has D-Bus methods but needs the Akonadi stack. A graphical client is then the person's own view of the same IMAP account.

| | Thunderbird | Evolution | Geary | KMail |
|---|---|---|---|---|
| F44 version | 158.0-2, `updates` | 3.60.2, `updates` | 46.0-9 | 25.12.3 per mdapi, spec at 26.08.2 (not verified which is stable) |
| Installed size | 385.8 MB | 18.5 + data server 9.5 + langpacks 41.9 MB, plus WebKitGTK 104.1 MB | 12.9 MB plus WebKitGTK | 20.1 MB plus Akonadi, MariaDB (81.8 MB), `qt6-qtwebengine` (290.8 MB) |
| License | MPL-2.0 OR GPL-2.0-or-later OR LGPL-2.0-or-later ([spec](https://src.fedoraproject.org/rpms/thunderbird/raw/f44/f/thunderbird.spec)) | GPL-2.0-or-later AND GFDL-1.3-or-later ([spec](https://src.fedoraproject.org/rpms/evolution/raw/f44/f/evolution.spec)); data server LGPL-2.0-or-later | LGPL-2.1-or-later ([COPYING](https://gitlab.gnome.org/GNOME/geary/-/raw/main/COPYING)) | GPL, many licenses ([spec](https://src.fedoraproject.org/rpms/kmail/raw/f44/f/kmail.spec)) |
| Wayland | Launcher sets `MOZ_ENABLE_WAYLAND=1` by default (spec line 579) | GTK3, no statement | GTK3, no statement | Qt6; 12 open kmail2 Wayland bugs, e.g. [468590](https://bugs.kde.org/show_bug.cgi?id=468590) |
| Calendar | Built in | Yes, EDS CalDAV | None | Yes, via Akonadi |
| Driving | `-compose`, `-file`, D-Bus `OpenURL`, WebExtensions | `mailto:`, EDS D-Bus for calendar and contacts | `mailto:`, GApplication | D-Bus `org.kde.kmail` (`openComposer`, `checkMail`) |

### Thunderbird

- **License.** The RPM tag is a tri-license that includes MPL-2.0. Upstream has no top-level license file; its [README](https://hg.mozilla.org/comm-central/raw-file/tip/README.md) points to `toolkit/content/license.html`, which was not audited, and says no rights are granted to Mozilla's trademarks. Source headers say MPL 2.0 ([nsMailApp.cpp](https://hg.mozilla.org/comm-central/raw-file/tip/mail/app/nsMailApp.cpp)).
- **Arabic and RTL.** The Gecko engine does bidi and shaping; the open bugs are about interface and direction controls, roughly ten relevant in a sample: [464436](https://bugzilla.mozilla.org/show_bug.cgi?id=464436) bidi mail UI and direction buttons in the composer (NEW, filed 2008); [1873711](https://bugzilla.mozilla.org/show_bug.cgi?id=1873711) wrong direction for RTL text in views and inputs (UNCONFIRMED); [1774767](https://bugzilla.mozilla.org/show_bug.cgi?id=1774767) shortcuts and auto-alignment for RTL input (UNCONFIRMED); [373479](https://bugzilla.mozilla.org/show_bug.cgi?id=373479) reply and forward buttons not flipped in an RTL interface (NEW). A third-party add-on, [BiDi Mail UI](https://github.com/eyalroz/bidimailui), adds paragraph-direction controls; ScarfeeOS would have to ship or rewrite it, and its license was not checked.
- **Memory and disk.** 385.8 MB installed with its own Gecko; runtime memory not measured.
- **Wayland.** Native by default through Fedora's launcher. 19 open Thunderbird bugs mention Wayland, e.g. [1901446](https://bugzilla.mozilla.org/show_bug.cgi?id=1901446) generic window icon (REOPENED).
- **Driving.** Command-line options are `-compose "to=...,subject=...,body=...,attachment=..."`, `-file x.eml|x.ics`, `-calendar`, `-addressbook`. A running instance is reached through the Gecko D-Bus remote `OpenURL` ([nsDBusRemoteServer.cpp](https://hg.mozilla.org/mozilla-central/raw-file/tip/toolkit/components/remote/nsDBusRemoteServer.cpp)); the Fedora bus name is derived by reading format strings, not run. A WebExtension would need a bridge ([compose API](https://webextension-api.thunderbird.net/en/latest/compose.html)); whether Fedora's build enables Marionette is not verified.
- **Not verified:** runtime behaviour, RAM, real Arabic mail rendering, third-party license inventory.

### Others

- **Evolution** is the second choice: no open RTL or Arabic issue among its 337 open GNOME GitLab issues (checked by text search, which may mean few reports), and its composer code handles `direction` ([e-editor.js](https://gitlab.gnome.org/GNOME/evolution/-/raw/master/data/webkit/e-editor.js)). Costs: several resident Evolution-Data-Server processes ([spec](https://src.fedoraproject.org/rpms/evolution-data-server/raw/f44/f/evolution-data-server.spec) lines 356-373), its own CalDAV store alongside ScarfeeOS's meetings manager, GPL mix, 104 MB WebKitGTK, and the Wayland and composer direction toggle are not documented. The upstream COPYING says files without a header are LGPLv2 or LGPLv3, the Fedora tag says GPL-2.0-or-later; which files are GPL was not resolved ([COPYING](https://gitlab.gnome.org/GNOME/evolution/-/raw/master/COPYING)).
- **Geary** is light and LGPL-2.1+, but the latest release tag is 46.0 from 2024-05-20 ([tags](https://gitlab.gnome.org/api/v4/projects/GNOME%2Fgeary/repository/tags)), it has no calendar, and it still uses GTK3 and `libpeas-1`. No RTL evidence either way (0 relevant of 303 open issues).
- **KMail** has the best D-Bus surface (`org.kde.kmail`, [kmkernel.h](https://invent.kde.org/pim/kmail/-/raw/master/src/kmkernel.h)) but the heaviest stack: Akonadi processes, MariaDB as the Fedora default backend ([akonadi-server.spec](https://src.fedoraproject.org/rpms/akonadi-server/raw/f44/f/akonadi-server.spec)), Qt WebEngine. Akonadi memory reports exist ([395131](https://bugs.kde.org/show_bug.cgi?id=395131), [471864](https://bugs.kde.org/show_bug.cgi?id=471864); anecdotes). Not recommended for 16 GB shared with a model.
- **aerc** (MIT, [LICENSE](https://git.sr.ht/~rjarry/aerc/blob/master/LICENSE), 19.1 MB) and **NeoMutt** (GPL-2.0-or-later, [README](https://raw.githubusercontent.com/neomutt/neomutt/main/README.md)) are terminal programs with no bidi of their own; the closed NeoMutt thread [#1671](https://github.com/neomutt/neomutt/issues/1671) says message bodies render correctly through an external `fribidi` display filter, but the message index is not shown RTL. aerc's tracker was behind a bot challenge, so no aerc RTL information. Neither is an interface for ordinary users.

### Choice

Thunderbird as the graphical client, with the Rust core doing IMAP and SMTP for the assistant. Thunderbird's built-in calendar duplicates the CalDAV meetings manager, so the user would run two calendars unless it is disabled.

## Media

### Codec state in Fedora 44 (applies to every player)

- Fedora's `ffmpeg` builds `ffmpeg-free` with `--disable-decoder="h264,hevc,vc1,vvc"` and decoder and encoder allow-lists ([ffmpeg.spec](https://src.fedoraproject.org/rpms/ffmpeg/raw/f44/f/ffmpeg.spec), [enable_decoders](https://src.fedoraproject.org/rpms/ffmpeg/raw/f44/f/enable_decoders)). Royalty-free formats (Opus, Vorbis, FLAC, VP8, VP9, AV1) play out of the box. H.264 and H.265 do not, and ScarfeeOS bundles neither, so this fits rule 4.
- **Unresolved:** the allow-lists include `aac` and `mp3` decoders, but Fedora's own guide says MP3 and AAC need RPM Fusion ([guide](https://docs.fedoraproject.org/en-US/quick-docs/installing-plugins-for-playing-movies-and-music/), last review 2026-05-10). Both are primary sources and they disagree; nothing could be played here to settle it.
- This is the same AAC item already open in [ip-and-licensing.md](ip-and-licensing.md): `libavcodec-free` and `pipewire-libs` both require `libfdk-aac.so.2` ([libavcodec-free](https://mdapi.fedoraproject.org/f44/pkg/libavcodec-free), [pipewire-libs](https://mdapi.fedoraproject.org/f44/pkg/pipewire-libs)); the library is licensed under [FDK-AAC](https://src.fedoraproject.org/rpms/fdk-aac-free/raw/f44/f/fdk-aac-free.spec) whose [NOTICE](https://raw.githubusercontent.com/mstorsjo/fdk-aac/master/NOTICE) says "NO EXPRESS OR IMPLIED LICENSES TO ANY PATENT CLAIMS ... ARE GRANTED". That the PipeWire AAC path is compiled in is inferred from the dependency and the lack of a disable flag in the [spec](https://src.fedoraproject.org/rpms/pipewire/raw/f44/f/pipewire.spec). This page does not settle it.

| | mpv | Celluloid | Showtime | Haruna | VLC |
|---|---|---|---|---|---|
| F44 version | 0.41.0-5, stable | 0.30-1, stable | 50.0-1, stable | 1.8.1-1, stable | 3.0.24-1, stable |
| Installed size | 4.8 MB (libmpv 3.2) | 1.3 MB + GTK4, libadwaita, libmpv | 0.7 MB + GTK4 30.4, GStreamer 33+ | 5.6 MB + Qt6, KF6 (`qt6-qtdeclarative` 63.4) | `vlc-libs` 45.5 + plugins |
| License | GPL-2.0-or-later AND LGPL-2.1-or-later AND BSD, ISC, MIT ([spec](https://src.fedoraproject.org/rpms/mpv/raw/f44/f/mpv.spec)) | GPL-3.0-or-later | GPL-3.0-or-later | mixed, source GPL-3.0-or-later | GPL-2.0-or-later AND LGPL-2.1-or-later AND BSD ([spec](https://src.fedoraproject.org/rpms/vlc/raw/f44/f/vlc.spec)) |
| Wayland | Native (`-Dwayland=enabled`) | GTK4, not tested | GTK4, not tested | Qt6, not tested | X11 libraries required; native not verified |
| Driving | JSON IPC socket, MPRIS plugin | GApplication, MPRIS in source | Full MPRIS in source | MPRIS, single instance | Lua, `vlc-cli`; not read |

### mpv

- **License and rule 2.** The Fedora build is the GPL build: upstream's [Copyright](https://raw.githubusercontent.com/mpv-player/mpv/v0.41.0/Copyright) says GPL v2 or later by default and LGPL only with `-Dgpl=false`; the spec never sets that and enables GPL-only features such as `-Dcaca`, `-Ddvdnav` and `-Dx11` ([meson.options](https://raw.githubusercontent.com/mpv-player/mpv/v0.41.0/meson.options)). So **`libmpv` in Fedora is GPL: ScarfeeOS code must not link it**. Run `mpv` as a separate process and talk to it over the socket, as [ip-and-licensing.md](ip-and-licensing.md) already allows.
- **Arabic and RTL.** Subtitles and the on-screen display go through libass (ISC) with FriBidi and HarfBuzz; `--sub-shaper=complex` is the default ([options.rst](https://raw.githubusercontent.com/mpv-player/mpv/v0.41.0/DOCS/man/options.rst)). Open issues: [mpv #17605](https://github.com/mpv-player/mpv/issues/17605) an Arabic track name scrambles the track menu (opened 2026-03-19); [#12978](https://github.com/mpv-player/mpv/issues/12978) RTL display of subtitles (open since 2023, the proposed `force-rtl` option [PR 16609](https://github.com/mpv-player/mpv/pull/16609) depends on [libass PR 901](https://github.com/libass/libass/pull/901), both unmerged); [libass #538](https://github.com/libass/libass/issues/538) bidi differs from VSFilter for digits mixed with Arabic; [libass #737](https://github.com/libass/libass/issues/737) wrong font for the on-screen display on Fedora 39+, with Noto Sans Arabic chosen for digits (not checked on F44). Whether the on-screen controller is translated into Arabic was not verified.
- **Memory and disk.** 4.8 MB package but a wide dependency fan-out (ffmpeg libraries, Lua, SDL2, JACK, PipeWire); one process, no UI toolkit. `libmpv` weakly recommends `yt-dlp` (22 MB installed); disable weak dependencies if that matters. Runtime memory not measured.
- **Wayland.** Native; `vo=gpu-next` is the default, and the Wayland context is chosen automatically ([vo.rst](https://raw.githubusercontent.com/mpv-player/mpv/v0.41.0/DOCS/man/vo.rst)). System-wide media keys are not implemented on X11 or Wayland, so the MPRIS plugin or the shell has to handle them. Confirmed here: the spec builds with Wayland support.
- **Driving.** `--input-ipc-server=PATH` opens a Unix socket speaking one JSON object per line (`{"command": ["loadfile", "x.mkv"]}`); `--idle=yes` keeps it alive without a file ([ipc.rst](https://raw.githubusercontent.com/mpv-player/mpv/v0.41.0/DOCS/man/ipc.rst), [options.rst](https://raw.githubusercontent.com/mpv-player/mpv/v0.41.0/DOCS/man/options.rst)). The doc is blunt that the protocol is "explicitly insecure": no authentication, and the `run` command can run arbitrary commands. So the socket belongs in `$XDG_RUNTIME_DIR` with user-only permissions (this advice is mine, not the docs'). `mpv-mpris` 1.2 (MIT, [LICENSE](https://raw.githubusercontent.com/hoyon/mpv-mpris/master/LICENSE)) adds the standard MPRIS interface for media keys. No pipe or socket was exercised.

### Others

- **Celluloid** (GTK4, GPL-3.0-or-later, [COPYING](https://raw.githubusercontent.com/celluloid-player/celluloid/v0.30/COPYING)) is the optional graphical front end: libadwaita mirrors the interface, and the reversed seekbar and shortcut bugs [#769](https://github.com/celluloid-player/celluloid/issues/769) and [#966](https://github.com/celluloid-player/celluloid/issues/966) were fixed by [PR 1055](https://github.com/celluloid-player/celluloid/pull/1055) in 0.30, which F44 ships (confirmed through the GitHub compare API). It links Fedora's GPL libmpv in-process. Passing `--mpv-input-ipc-server=` through to mpv and its MPRIS bus name were not verified.
- **Showtime** (GStreamer, GPL-3.0-or-later, [COPYING](https://gitlab.gnome.org/GNOME/Showtime/-/raw/main/COPYING)) has full MPRIS and RTL seek keys fixed ([#159](https://gitlab.gnome.org/GNOME/showtime/-/work_items/159)), but open reports are a poor fit for a shared-memory laptop: [#313](https://gitlab.gnome.org/GNOME/showtime/-/work_items/313) GPU memory grows after closing videos (15 GiB observed in one report), [#300](https://gitlab.gnome.org/GNOME/showtime/-/work_items/300) processes remain after closing, [#309](https://gitlab.gnome.org/GNOME/showtime/-/work_items/309) an MPRIS deadlock at startup on `showtime-50.0-1.fc44`. All are single user reports and none was triaged; they also lack ASS subtitle support ([#144](https://gitlab.gnome.org/GNOME/showtime/-/work_items/144)).
- **Haruna** (Qt6/KF6 over libmpv, [source header GPL-3.0-or-later](https://invent.kde.org/multimedia/haruna/-/raw/master/src/main.cpp)) adds a second toolkit stack. Its RTL seekbar bug [456005](https://bugs.kde.org/show_bug.cgi?id=456005) is fixed; no open RTL bug matched a title search.
- **VLC** is a real player in Fedora (the `vlc` package is a small metapackage over `vlc-libs`), with x264 and x265 off in Fedora. Open RTL reports, e.g. [25681](https://code.videolan.org/videolan/vlc/-/issues/25681) Arabic or Persian SubStation Alpha subtitles and [18371](https://code.videolan.org/videolan/vlc/-/issues/18371) "RTL support is lacking in most platforms"; X11 libraries are hard requirements of its Qt and video-out packages. Per rule 6 its name must not appear in anything ScarfeeOS ships.
- **Totem** is unsupported upstream in distribution packages ([README](https://gitlab.gnome.org/GNOME/totem/-/raw/master/README.md)); Showtime replaced it. Rejected.

### Choice

mpv, as a separate process driven over its JSON IPC socket, with `mpv-mpris` optional and Celluloid as the optional window. The ScarfeeOS shell can draw its own controls.

## Browser

Rule 5 asks for a WebView-based browser first, built without proprietary codecs, and a Gecko or Chromium build later. Fedora has no package that is exactly that, so this section compares the packages and the project's own option.

| | Own Tauri shell on `webkit2gtk4.1` | Epiphany (WebKitGTK 6.0) | Firefox | Chromium | Falkon | qutebrowser |
|---|---|---|---|---|---|---|
| F44 version | engine 2.54.2 | 50.6 (stable 50.6-1; 50.6-6 testing) | 159.0 | 156.0.8078.73 | 26.08.1 | 3.7.0-2 |
| Installed size | engine 104.1 + JavaScriptCore 18.0 MB | 11.4 + runtime 7.1, plus WebKitGTK 104.0, JSC 18.0, GTK4 30.4 | 283.2 + langpacks 45.0 MB | 328.6 + common 74.0 MB | 14.2 MB plus WebEngine 290.8 | 11.0 MB plus WebEngine 290.8 |
| License | Rust crates Apache-2.0 OR MIT; WebKit LGPL and BSD | GPL-3.0-or-later AND CC-BY-SA-3.0 AND CC0-1.0 | Tri-license with MPL-1.1 in the tag (not MPL-2.0) | BSD-3-Clause main, mixed bundled tag | GPL-3.0-or-later AND BSD | GPL-3.0-only AND MIT |
| Wayland | GTK3 backend, not tested | GTK4, native by inference | Default since Firefox 121 | Documented Ozone backend | Not verified | Not verified |
| Driving | Own IPC | WebKitWebDriver (`--automation-mode`) | Marionette, WebDriver BiDi, D-Bus `OpenURL` | CDP, chromedriver | CLI, remote debugging flag | `:command` over local socket |

### Own Tauri shell on `webkit2gtk4.1`

- Best fit for rules 2, 5 and 6: `wry` and `tauri` are Apache-2.0 OR MIT ([wry Cargo.toml](https://github.com/tauri-apps/wry/blob/dev/Cargo.toml)), WebKitGTK is LGPL and BSD in a shared library, there is no third-party name, and the engine takes its codecs from the system GStreamer rather than a bundled set. Tauri on Linux uses `webkit2gtk` ([webview versions](https://v2.tauri.app/reference/webview-versions/)) and its Fedora prerequisite is `webkit2gtk4.1-devel` ([prerequisites](https://v2.tauri.app/start/prerequisites/)).
- **Fedora's `webkit2gtk4.1` has no WebDriver.** The [spec](https://src.fedoraproject.org/rpms/webkitgtk/raw/f44/f/webkitgtk.spec) builds the GTK3 flavour with `-DENABLE_WEBDRIVER=OFF` (line 334) and `WebKitWebDriver` is in the `webkitgtk6.0` file list (line 390). Automation of a Tauri browser needs its own IPC, which the Rust core can provide.
- It is a browser to build. WebKit's Arabic bugs become ours: [255972](https://bugs.webkit.org/show_bug.cgi?id=255972) some Arabic fonts are not shaped correctly in WebKitGTK (NEW, since 2023), [236117](https://bugs.webkit.org/show_bug.cgi?id=236117) `<input type=date>` unintelligible in Arabic (NEW), and caret bugs in RTL fields such as [220361](https://bugs.webkit.org/show_bug.cgi?id=220361) (NEW). A sample of 187 titled bugs remains after filtering, many being test maintenance. Security: WebKitGTK published six advisories in 2026 so far, the latest on 29 September ([security page](https://webkitgtk.org/security.html)); the shell has to follow Fedora's updates.

### Epiphany

- Same engine, packaged and maintained by others. 11.4 MB itself. GPL-3.0-or-later, so a separate program only. The upstream README recommends Flatpak and says distribution packages are often outdated ([README](https://gitlab.gnome.org/GNOME/epiphany/-/blob/main/README.md)); Fedora carries five CVE patches in the spec.
- Driving: `epiphany --automation-mode` starts "a private instance for WebDriver control" ([man page](https://gitlab.gnome.org/GNOME/epiphany/-/raw/main/data/epiphany.1.rst)), and `WebKitWebDriver` takes a `webkitgtk:browserOptions` capability with `binary` and `args`, so `binary=epiphany, args=["--automation-mode"]` should work. This is inferred; it was never run. Also `org.gnome.Epiphany` GApplication actions (new window and others) but no open-URL action.
- Arabic: GTK4 and libadwaita mirror the interface, 1 relevant open issue of 281 ([#216](https://gitlab.gnome.org/GNOME/epiphany/-/work_items/216)); page content has the WebKit bugs above.
- Rule 6: "GNOME Web" and "Epiphany" are third-party names.

### Gecko and Chromium

- **Firefox** is the best fit for the "later" Gecko step. Bidi and Arabic are mature (Core has hundreds of RTL and bidi hits in a keyword sample, mostly edge cases, e.g. [1938822](https://bugzilla.mozilla.org/show_bug.cgi?id=1938822) kashidas break letter connections on Linux, NEW), and it defaults to Wayland when available ([121 release notes](https://www.mozilla.org/en-US/firefox/121.0/releasenotes/)). Marionette and WebDriver BiDi are the automation routes ([remote docs](https://firefox-source-docs.mozilla.org/remote/index.html)); CDP ended in Firefox 141. `geckodriver` is not packaged in Fedora. The RPM links `fdk-aac-free` and recommends `ffmpeg-free`, so rules 4 and 5 need configuration. The name and logo are Mozilla marks ([LICENSE](https://raw.githubusercontent.com/mozilla/gecko-dev/master/LICENSE)). Not verified: the `--marionette` flag and port, Fedora's D-Bus app name.
- **Chromium** has a BSD-3-Clause core ([LICENSE](https://chromium.googlesource.com/chromium/src/+/main/LICENSE)), but a mixed RPM tag, about 400 MB installed, `proprietary_codecs=true` in the build flags ([spec](https://src.fedoraproject.org/rpms/chromium/raw/f44/f/chromium.spec)), and its Arabic tracker could not be read: issues.chromium.org serves only a script shell. No Chromium bug evidence is given. CDP needs `--remote-debugging-port` plus `--user-data-dir` from Chrome 136 ([note](https://developer.chrome.com/blog/remote-debugging-port)).
- **Falkon** and **qutebrowser** are GPL programs on Qt WebEngine (a Chromium; about 291 MB). qutebrowser has the best scriptable local command interface, but a keyboard-driven interface that does not suit the general user; Falkon has no control API beyond the command line and the debugging flag. Qt has 46 open Arabic or RTL issues, none in its WebEngine components ([Qt Jira](https://qt-project.atlassian.net), e.g. QTBUG-88716 Eastern Arabic numerals not recognised in Widgets). Neither is recommended.

### Choice

The research pass ranked the project's **own Tauri shell on `webkit2gtk4.1`** first: it fits rules 2, 5 and 6 best and avoids the trademark and GPL-app questions, but it is a browser to build and has no external WebDriver. **Epiphany (WebKitGTK 6.0)** is the Fedora package on the same engine and can be scripted today through WebKitWebDriver, so it is the fallback if the shell slips. This page recommends the shell with Epiphany as the stand-in; the owner decides (see open items). Firefox is the later Gecko step.

## Not tested

- Nothing was installed or run. No Fedora machine was used. No program was started, so no Wayland behaviour, window drawing, Arabic text, IPC socket, D-Bus call or command-line flag was exercised.
- No memory figure for any program was found or measured. Only package sizes are given.
- Bug lists are title or text samples on 2026-10-31. Single-reporter bugs (mostly UNCONFIRMED) were not reproduced.
- GitHub-hosted entries (mpv, libass, Celluloid, pandoc, NeoMutt) rest on the research pass; I re-checked only mpv #17605 and #12978 myself. GitHub's rate limit also cut the NeoMutt and VisiData searches short. The Chromium tracker was unreachable. aerc's tracker was behind a bot challenge.
- Bundled third-party licenses (Gecko, WebKitGTK, QtWebEngine, LibreOffice libraries) were not audited. For WebKitGTK the [spec](https://src.fedoraproject.org/rpms/webkitgtk/raw/f44/f/webkitgtk.spec) says its own tag was last updated at 2.42.2, so it is stale against 2.54.2.
- Arabic font coverage, Hijri behaviour, and Eastern Arabic digit behaviour were not checked at runtime anywhere.
- Fedora's current package for each version listed may have moved since the check; mdapi and Bodhi disagree for several packages (see the top table).

## Open items

- **Names (rule 6).** Using LibreOffice, Thunderbird, Firefox, Epiphany or mpv under their own names inside ScarfeeOS needs legal review. Mozilla's README and LICENSE say no trademark rights are granted. If ScarfeeOS ships them with its own names and designs, that is a separate question for the lawyer, and the Fedora packages could not simply be renamed. Naming them in docs is fine.
- **AAC (rule 4).** The open `fdk-aac-free` item in [ip-and-licensing.md](ip-and-licensing.md) also covers Fedora's ffmpeg, Firefox and Chromium builds, not only PipeWire. The Fedora guide and the ffmpeg spec disagree about MP3 and AAC; test on a real F44 machine.
- **Browser order.** Decide whether stage 6 builds the Tauri shell straight away or ships Epiphany first.
- **Mail model.** Decide that the assistant does IMAP and SMTP itself, and whether Thunderbird's calendar is disabled to avoid a second CalDAV store.
- **Image settings.** If LibreOffice is chosen: pick the GTK plugin explicitly, leave out Java, Base and the other VCL plugins, and set RTL document defaults.
- **Test on a real machine** before any decision: LibreOffice with Arabic documents (tables, DOCX, Impress slideshow on Wayland), Thunderbird composer direction, mpv with Arabic subtitles, Epiphany with `--automation-mode`, and RAM of each with a model loaded.

## Changes needed in other files

This page changes only itself. If the recommendations are accepted, [lifecycle.md](lifecycle.md) stage 6 and `ROADMAP.md` could link here, and [ip-and-licensing.md](ip-and-licensing.md) could list the productivity apps and the trademark question under its open items. No Arabic interface strings were added.
