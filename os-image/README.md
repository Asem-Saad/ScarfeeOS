# os-image

Fedora-based image build and installer. The installer's first screen is language selection (English / العربية); see ../docs/installer.md.

## Status: checked against Fedora 44 metadata, nothing built
- **No image has been built.** The definition has been validated against kiwi's schema but never run through a build.
- **Checked on 2026-10-30** against Fedora 44 (the newest stable release that day; Fedora 43 reaches end of life on 2026-12-02, Bodhi listed Fedora 45 as frozen, not released). Sources:
  - Release list and end-of-life dates: Fedora Bodhi, `https://bodhi.fedoraproject.org/releases/`.
  - Repository metadata: the `repodata/primary.xml.zst` files of `https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Everything/x86_64/os/` and `.../updates/44/Everything/x86_64/` (both URLs in `config.xml` exist and return metadata). Every name in `packages.txt` was looked up there (name, version, licence), and the `Requires:` of the install set were followed to see what gets pulled in.
  - The `generic-release` and `generic-logos` RPMs were downloaded from that repository and their files and licence texts read.
- **Package names:** all names in `[bootstrap]` and `[image]` exist in Fedora 44 except one, now fixed: `dnf` is no longer a package (it is a virtual name provided by `dnf5`), so the list now says `dnf5`.
- **kiwi schema:** validated with kiwi 11.1.6 from PyPI (installed in a Python virtual environment, no root): `kiwi-ng image info --description os-image`. The original file failed with two errors: `bootloader` is not an attribute of `<type>` (it is a child element `<bootloader name="grub2"/>`), and `initrd_system` is only allowed for `oem`, `pxe` and `kis` types, not `iso`. The original `schemaversion="7.5"` was out of date; kiwi converts it on load, but the current version is `8.5`, now set. After the fixes the same command loads the description and selects build type `iso` with no errors. `jing` was not installed, so only kiwi's own library error log was available. Fedora 44 itself ships kiwi 11.1.1, whose schema was not checked; run the check again with that version.
- **Ignore list:** `gstreamer1-libav` is really `gstreamer1-plugin-libav` in Fedora (fixed), and `fdk-aac-free` could not stay on it (see "Still open").
- The automated check is `test_image_definition.py`: policy rules, and kiwi schema validation when `kiwi-ng` is on `PATH` (skipped otherwise).

## Files
| File | Purpose |
|------|---------|
| `config.xml` | kiwi image description: x86_64 ISO, UEFI and BIOS boot, English and Arabic locales |
| `packages.txt` | Readable package list; must match `config.xml` (the test checks this) |
| `scarfeeos-release/` | RPM spec and sources for `scarfeeos-release` (os-release, issue, service presets) |
| `scarfeeos-logos/` | RPM spec and the original SVG logo for `scarfeeos-logos` |
| `test_image_definition.py` | Well-formedness, policy checks, spec and logo checks, an `rpmbuild` run if it is installed, and kiwi schema validation if kiwi is installed |

## Build steps (untested)
Needs a Fedora machine with root access.

1. Install kiwi: `sudo dnf install kiwi-cli` (the package exists in Fedora 44, version 11.1.1). kiwi also needs the `kiwi-systemdeps-*` packages for ISO building; `kiwi-cli` pulls in the core set.
2. Edit `config.xml` only if needed: confirm the Fedora release (44) in the two `<repository>` paths is still supported.
3. Install `rpm-build` and `createrepo_c`, then build the two ScarfeeOS packages into the local repository that `config.xml` reads (`repo/`, not committed):
   ```
   for n in release logos; do rpmbuild -bb --define "_topdir $PWD/rpmbuild" --define "_sourcedir $PWD/os-image/scarfeeos-$n" os-image/scarfeeos-$n/scarfeeos-$n.spec; done
   mkdir -p os-image/repo && cp rpmbuild/RPMS/noarch/*.rpm os-image/repo/ && createrepo_c os-image/repo
   ```
4. Build: `sudo kiwi-ng --type iso system build --description os-image --target-dir /tmp/scarfeeos-build`
5. The ISO is written to the target directory. Boot it in a VM before anything else.
6. Check: `python3 os-image/test_image_definition.py` (this works without kiwi; with kiwi on `PATH` it also validates the schema).

A first real build may still fail: dependency resolution, bootloader and live-ISO options were not exercised.

## What the image contains
- **Fonts** (docs/i18n-and-arabic.md rule 5): Noto Naskh Arabic, Noto Kufi Arabic, IBM Plex Sans Arabic (all three font packages exist in Fedora 44 under these names, licence OFL-1.1), plus Noto Sans for Latin text. HarfBuzz and FriBidi for shaping and bidi.
- **Arabic input:** `ibus` with `ibus-m17n` and `ibus-typing-booster`. The plain Arabic keyboard layout comes from the system keyboard data, not from ibus. `hunspell-ar` for spelling.
- **Locales:** `en_US` and `ar_SA` are both installed. Which one is the default is decided by the installer's language step.
- **Media:** only royalty-free codec libraries (AV1, VP9, Opus, Vorbis, FLAC, Ogg). No H.264 or H.265 packages are installed, and they are on the `<ignore>` list so they are not pulled in as dependencies. No `mpv` or `ffmpeg-free` yet, and `ffmpeg-free`, `libavcodec-free` and `gstreamer1-plugin-libav` are on the `<ignore>` list: whether Fedora's build includes patent-licensed decoders was not checked. The names that Fedora does not carry (`ffmpeg`, `x264`, `x265`, `openh264`, `libavcodec-freeworld` and others live in RPM Fusion or Cisco's repository) are kept on the ignore list as a guard.
- **Base:** kernel, systemd, NetworkManager, Bluetooth, PipeWire, power-profiles-daemon, Mesa with Vulkan (for llama.cpp on the Full tier).

## Branded packages (docs/ip-and-licensing.md rule 6)
These carry Fedora names or logos and are not installed. Replacements:

| Fedora package | Used for | Replacement |
|----------------|----------|-------------|
| `fedora-release`, `fedora-release-common`, `fedora-release-identity-*` | `/etc/os-release`, issue files, product name | `scarfeeos-release` (`scarfeeos-release/`, Apache-2.0). `generic-release` was checked and rejected (below) |
| `fedora-logos`, `fedora-logos-classic`, `fedora-logos-httpd` | Logos, boot splash, installer and desktop branding | `scarfeeos-logos` (`scarfeeos-logos/`): one original SVG mark only. `generic-logos` was checked and rejected (below). No boot splash, installer or wallpaper artwork yet |
| Fedora wallpaper packages (`fNN-backgrounds-*`) | Wallpapers | Not installed; original wallpaper needed |
| `fedora-bookmarks`, `fedora-chromium-config` | Browser defaults | Not installed |
| `fedora-flathub-remote`, `fedora-third-party`, `fedora-workstation-repositories` | Third-party repositories | Not installed |

### `generic-logos` 18.0.0-27.fc44: rejected
Licence in the repository metadata: `GPL-2.0-only AND LicenseRef-Callaway-LGPLv2+`. The package's own README says it exists for people who cannot meet the Fedora logo trademark rules, but its contents (downloaded and inspected 2026-10-30) are not usable:
- It ships a **KDE 4 logo** (`/usr/share/kde4/apps/ksplash/Themes/Leonidas/2048x1536/logo.png`). The bundled `COPYING-kde-logo` allows the logo only to refer to KDE itself. That is a third-party trademark, which rule 6 forbids.
- Its main image (`system-logo-white.png`, `fedora-logo-small.png`) is a cartoon hot dog mascot, not a neutral mark.
- Files are still named `fedora-logo*.png`, `fedora-logo-sprite.svg`, `fedora-logo-icon.svg`, `bootloader/fedora.icns` and `fedora.vol`, so the Fedora name would appear in the image's file tree.
- Some images are derived from GNOME icons under GPL-2.0 (per its `COPYING`), which is outside the preferred licences of rule 2 (artwork, not linked code, but it would still need a notice).

So `generic-logos` and `generic-logos-httpd` are not installed and are on the `<ignore>` list. `scarfeeos-logos` replaces them: a plain scarf mark in two colours (teal and amber, rectangles only), drawn for this project and not copied or traced from any existing logo. It is installed as the icon `scarfeeos-logo-icon`, which the `LOGO` field of os-release names. It is not rendered at small sizes yet; whether it reads well as a 16 px icon was not checked.

### `generic-release` 44-0.2: rejected, replaced by `scarfeeos-release`
Licence: MIT (Red Hat copyright, `LICENSE` file inside `generic-release-common`). Problems found in the downloaded RPMs:
- `generic-release-common` **requires `fedora-repos(44)`**, so `fedora-repos` and `fedora-gpg-keys` (both MIT) are installed whatever the ignore list says. The Fedora name therefore appears in `/etc/yum.repos.d/` and in repository IDs.
- Its `/usr/lib/os-release` has `ID_LIKE=fedora`, joke placeholder URLs for `HOME_URL`, `SUPPORT_URL`, `BUG_REPORT_URL` and `PRIVACY_POLICY_URL` (a novelty website, a song title page on Wikipedia, a YouTube link, `nsa.gov`), and the name "Generic". It also defines the RPM macros `%fedora` and `%dist` as `.fc44`.

### `scarfeeos-release`
Built from `scarfeeos-release/scarfeeos-release.spec` (Apache-2.0, project URL `https://github.com/Asem-Saad/ScarfeeOS`). Checked with rpmbuild 4.18.2 and rpmlint 2.10.0 (Ubuntu's rpm built in user space, not Fedora's; `rpmlint` could not run its spec check because it needs `/usr/bin/rpmspec`). Both specs build; rpmlint reports no errors beyond `hardcoded-library-path` for `%{_prefix}/lib` (os-release must live in `/usr/lib`), plus missing signature, Packager and Group tags, and an `invalid-license` warning for `Apache-2.0` that was not investigated.
- **Identity:** `/usr/lib/os-release` names ScarfeeOS and has real GitHub URLs (home, documentation, issues). It sets no `ID_LIKE`, no privacy policy URL (none exists yet), and no Red Hat bugzilla fields. Without `ID_LIKE=fedora`, tools that look for it will not treat the image as Fedora-like; nothing in the package list was checked for that.
- **Version:** the package version is `44`, the base release, because dnf takes `$releasever` from the package that provides `system-release` and the repository URLs are keyed on it (not run: no dnf was available). The ScarfeeOS version, 0.0.1, is in os-release, the issue and release files and the CPE name; the test checks they match `config.xml`.
- **Services:** it ships its own short service presets (NetworkManager, Bluetooth, polkit, power-profiles-daemon, dbus-broker, PipeWire and WirePlumber for users, everything else off) instead of Fedora's 284-line list. Unit names were checked against the Fedora 44 RPM file lists; whether the live image boots with them was not tried. No time-sync service is in the package list, so none is enabled.
- **Not carried over from `generic-release`:** the `/etc/redhat-release` and `/etc/fedora-release` files, and the `%fedora` and `%dist` RPM macros. Packages rebuilt on a ScarfeeOS system that expect `%fedora` or `%dist` will need them; not decided.
- **Dependencies on Fedora packages that remain:** `Requires: fedora-repos(44)`. That pulls in `fedora-gpg-keys` (MIT), and `fedora-repos` itself requires `system-release(44)`, which `scarfeeos-release` provides. It also installs the repository files `fedora.repo`, `fedora-updates.repo`, `fedora-updates-testing.repo` and `fedora-cisco-openh264.repo`, so repository IDs and names say Fedora in dnf output, and **`fedora-cisco-openh264` is enabled by default** in that package (the other `fedora-cisco-openh264-*` repositories are off). Cisco's repository serves a patent-licensed H.264 library; nothing in the image installs from it, but docs/ip-and-licensing.md rule 4 should be checked against shipping it enabled.
- **Can `fedora-repos` be replaced?** The repository files, yes: a ScarfeeOS-owned `scarfeeos-repos` package with its own repo IDs and names, pointing at Fedora's mirror metalinks, would remove the name from dnf output (the URLs still say Fedora; that is the trademark review above). The signing keys, no: packages from Fedora are signed with Fedora's keys, so `fedora-gpg-keys` stays unless ScarfeeOS builds and signs every package itself. Not done here.

Still open, for legal review:
- `fedora-repos` and `fedora-gpg-keys` are normally needed for updates, and the Fedora name appears in repository definitions and URLs. The `<repository>` URLs in `config.xml` name Fedora. Whether the image can ship updates from Fedora's servers, and how, needs the Fedora trademark review already listed in docs/ip-and-licensing.md.
- `scarfeeos-release` declares `Conflicts: fedora-release` (as `generic-release` did), and a Requires-closure check over the repository metadata (unique providers only, not a real depsolve) found no package in the install set that needs `fedora-release`. A real depsolve has not been done.
- **AAC:** `pipewire-libs` (pulled in by `wireplumber`) requires `libfdk-aac.so.2`, so `fdk-aac-free` (licence `FDK-AAC`) cannot be excluded without removing PipeWire. It was therefore taken off the `<ignore>` list, which would otherwise make the build fail to resolve. It is a library dependency of the Bluetooth audio code, not something ScarfeeOS-owned code uses, but rule 4 says AAC only after the patent status is verified: **decision needed from the maintainer and legal review** (accept it, or build PipeWire without it).
- **H.264 stub:** `libheif` (pulled in through GTK 4 via `ibus-m17n`) requires `libopenh264.so.8`, satisfied by Fedora's `noopenh264` 2.6.0-4.fc44 (licence `BSD-2-Clause and LGPL-2.1-or-later`), a stub library with no codec in it. It is not the Cisco OpenH264 binary. Not removed; noted here so nobody is surprised.
- The repositories in `config.xml` set no `gpgcheck` options. Signature checking for the build was not looked at.
- The bootloader and shim may still use `fedora` as the EFI directory or boot ID. Not checked.
- No upstream package, kernel or licence notices review has been done.

## Language selection comes first
docs/installer.md requires the very first installer screen to be the bilingual "Select your language / اختر لغتك" screen, with nothing else possible before it. **None of this is built.** The plan:

1. The ISO boots to a minimal Wayland session that starts only the ScarfeeOS installer front-end. Both locales, all the fonts above and ibus are already on the image, so the first screen renders correctly before any choice.
2. The front-end's first screen shows the two buttons, English and العربية. Its strings go through Fluent like the rest of the project (`locales/`).
3. The choice is written to a small answers file. It sets the system locale, the keyboard layout (with the other language's layout added as an option), the default AI conversation and speech language, and the calendar and numeral defaults, as listed in docs/installer.md. The rest of the installer then switches to that language, with RTL layout for Arabic.
4. Later steps (disk and account, hardware check and AI tier, optional Claude key, finish) follow, and the actual installation is done by existing Fedora tooling driven by the answers file.

Not decided yet: whether the install back-end is Anaconda (using a kickstart generated from the answers file) or something smaller. Anaconda's own first screen is a language list, but it is Fedora-branded and is not the two-button screen in docs/installer.md, so it is not a substitute.

## Not built yet
- Any image (this definition has been schema-validated by kiwi but never built).
- Installer front-end, the language screen, answers file, and the install back-end.
- Desktop session: the shell (`../shell/`) does not exist yet, so no compositor or desktop is in the package list and the live image has no graphical session or login defined.
- Wallpapers, boot splash and installer artwork (only the one SVG logo exists).
- A `scarfeeos-repos` package (see above) and signing of the ScarfeeOS packages.
- Root and user accounts (the installer's account step creates them).
- A system keyring package for the Claude API key (not chosen).
- Tier-specific content: model files, llama.cpp and speech engines are not in the image.
- SBOM and licence check for the image (rule 2).
