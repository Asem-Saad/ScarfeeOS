# os-image

Fedora-based image build and installer. The installer's first screen is language selection (English / العربية); see ../docs/installer.md.

## Status: checked without root against Fedora 44 metadata, nothing built
- **No image has ever been built.** kiwi needs root, which the engineer who prepared the first build did not have. On 2026-10-31 the definition was checked as far as it can be without root; `build-image.sh` is the script for the first real build. "What was proved" and "What only a root build can prove" below say exactly where the line is.
- **Fedora release:** 44, the newest stable release on 2026-10-30 (Fedora 43 reaches end of life on 2026-12-02; Bodhi listed Fedora 45 as frozen, not released, Bodhi: `https://bodhi.fedoraproject.org/releases/`). Re-check before a real build; `build-image.sh` fails early if the two repository URLs in `config.xml` stop answering.
- **Package names and licences:** looked up in the `primary.xml.zst` of `https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Everything/x86_64/os/` and `.../updates/44/Everything/x86_64/` (see "What was proved"). `dnf` is no longer a package (it is a virtual name provided by `dnf5`), so the list says `dnf5`. `gstreamer1-libav` is really `gstreamer1-plugin-libav` in Fedora (fixed), and `fdk-aac-free` could not stay on the ignore list (see "Still open").
- The automated check is `test_image_definition.py`: policy rules, the settings listed under "Fixes from the first-build review", and kiwi schema validation when `kiwi-ng` is on `PATH` (skipped otherwise).
- **Second pass, 2026-10-30:** `scarfeeos-repos` and the desktop session were added and checked against the same metadata; the `fedora-repos` 44-2 RPM was downloaded and its repository files read. See "scarfeeos-repos" and "Desktop session" below.

### What was proved (2026-10-31, no root, no Fedora machine)
- **Every package exists and the whole set resolves.** The base and updates `primary` and `filelists` metadata for Fedora 44 were loaded into libsolv (`python3-solv`, the library behind dnf, taken from Ubuntu packages and run in user space) and the `[bootstrap]` and `[image]` lists solved with the `[ignore]` list locked out, weak dependencies off (kiwi's default), and architectures `x86_64`, `i686` and `noarch`. Result: no missing package and no conflict. The bootstrap set plus `dnf5` is 97 packages; the full image is 445 packages (the three `scarfeeos-*` RPMs were built with rpmbuild and added as a local repository). About 550 MiB to download and 1.8 GiB installed, from the `size` fields of the metadata. Nine names on the ignore list are not in Fedora 44 at all (`openh264`, `x264`, `x265`, `ffmpeg` and others, as intended: they are guards).
- **`config.xml` validates against kiwi 11.1.1's own schema.** The `python3-kiwi` 11.1.1 RPM (the version Fedora 44 ships) was downloaded and unpacked without installing it, and `kiwi.rng` from it checked with lxml: valid. kiwi 11.1.1 also loads the description (`kiwi-ng image info --description os-image`) and kiwi's own runtime checker (`RuntimeChecker`, its checks for build type `iso`, except those that need a built root, which were skipped) passes.
- **The shipped RPM specs build** with rpmbuild 4.18.2 (Ubuntu's, in user space), the same `rpmbuild -bb` call the script makes; see "scarfeeos-release". Fedora's rpmbuild was not used.
- **File names the bootloader step looks for exist** in the packages: `grub2-mkimage` (in `grub2-tools`, installed through `grub2-pc`) and `gcdx64.efi` (in `grub2-efi-x64-cdboot`).

### Fixes from the first-build review (2026-10-31)
Reading kiwi 11.1.1's source against the old definition found four things that would very probably have stopped the first build:
1. **`<packagemanager>dnf4</packagemanager>` replaced by `dnf5`.** With `dnf4`, kiwi adds the package `dnf` to the bootstrap set and then runs `dnf4` or `dnf-3` inside the new root. In Fedora 44 `dnf` is provided by `dnf5`, and `dnf4` comes from `python3-dnf`, which the image does not list, so the second phase would have no tool to run. With `dnf5` kiwi uses `dnf5 --installroot` for the bootstrap and `chroot dnf5` for the image, and `dnf5` is in the image list.
2. **`mediacheck="true"` removed.** kiwi's default tool for it is `checkmedia`/`tagmedia`, which are not in Fedora 44 (only `isomd5sum` is), and kiwi's runtime check stops the build on that. If a media check is wanted, set `media_tag_tool: isomd5sum` in `/etc/kiwi.yml` on the build host and put `mediacheck="true"` back.
3. **`<release-version>44</release-version>` added.** Without it kiwi passes `--releasever=0`, so `$releasever` expands to `0` in the repository files the image carries and in the build.
4. **`grub2-efi-x64-cdboot` added.** For an ISO, kiwi looks for `gcdx64.efi` first, and that file is in this package, not in `grub2-efi-x64`.

BIOS boot on the same ISO: `firmware="uefi"` means kiwi sets up UEFI boot, and as far as the source shows it also adds the BIOS modules (its `efi_csm` default is on, and `grub2-pc` is in the list), but that was only read, not run. Check it in the boot tests below.

### What only a root build can prove
- That `dnf5` really installs the set in a chroot: rpm scriptlets, file conflicts the metadata does not show, signature handling (kiwi build repositories are used without `gpgcheck`).
- That `dracut` builds an initrd with the `kiwi-live` module and that the live system mounts its squashfs with the overlay.
- That the GRUB image, the shim chain and the El Torito/EFI layout of the ISO are correct, and BIOS and UEFI booting.
- That the desktop, services, fonts, input method, Arabic rendering and everything else in "What the image contains" work.
- The real disk use and build time: the numbers in `build-image.sh` are estimates.
- That the build host (Fedora 44's own kiwi 11.1.1 with `dnf5`) behaves as read from the source. `build-image.sh` was run on Ubuntu only: its argument handling and prerequisite messages, and its whole flow with a stub `kiwi-ng` (in a user namespace posing as root and Fedora), which also ran the three real `rpmbuild` calls. Real `kiwi-ng`, `dnf5`, `createrepo_c` and `xorriso` never ran, and `shellcheck` was not available. One bug found that way: `kiwi-ng --version` exits 1 on 11.1.1, so the script ignores its status.

## Files
| File | Purpose |
|------|---------|
| `config.xml` | kiwi image description: x86_64 ISO, UEFI and BIOS boot, English and Arabic locales |
| `packages.txt` | Readable package list; must match `config.xml` (the test checks this) |
| `scarfeeos-release/` | RPM spec and sources for `scarfeeos-release` (os-release, issue, service presets) |
| `scarfeeos-repos/` | RPM spec and the repository file for `scarfeeos-repos` (replaces `fedora-repos`) |
| `scarfeeos-logos/` | RPM spec and the original SVG logo for `scarfeeos-logos` |
| `build-image.sh` | The first-build script (root, Fedora): checks prerequisites, builds the three RPMs into `repo/`, runs kiwi, writes the ISO and its checksum |
| `test_image_definition.py` | Well-formedness, policy checks (including: no enabled Cisco or H.264 repository), spec and logo checks, an `rpmbuild` run if it is installed, and kiwi schema validation if kiwi is installed |

## Build steps (untested)
Needs a Fedora machine (a VM or a privileged container works) with root, `x86_64`, about 10 GiB free for the output and 2 GiB in `/var/cache/kiwi`, and internet access.

1. Install the tools: `sudo dnf install kiwi-cli kiwi-systemdeps-core kiwi-systemdeps-filesystems kiwi-systemdeps-bootloaders kiwi-systemdeps-iso-media rpm-build createrepo_c curl`
2. Run: `sudo os-image/build-image.sh /path/to/output`

The script checks everything it needs first (root, Fedora, `x86_64`, the tools, kiwi 10 or newer, disk space, that the two Fedora repositories answer) and lists all problems before it changes anything. Then it builds `scarfeeos-release`, `scarfeeos-repos` and `scarfeeos-logos` into `os-image/repo/` (not committed), runs `createrepo_c` on it, and runs `kiwi-ng --type iso system build`. Output, all inside the path you give: `ScarfeeOS.x86_64-0.0.1.iso` and its `.sha256` (the name is kiwi's, not checked), `kiwi.log`, and `build/`, kiwi's work tree. Run it with `--help` to see the same facts.

If the build stops, send `kiwi.log` (and the last 40 lines of the screen). Build failures are expected on a first try, because none of the root-only steps listed above has run.

3. Boot-test it (next section).
4. Check the definition files: `python3 os-image/test_image_definition.py` (works without kiwi; with kiwi on `PATH` it also validates the schema).

## Boot-test checklist
Run QEMU first; a laptop only after QEMU boots. Write down what you see in each step, including what failed. Nothing here has been run yet.

**QEMU (UEFI), on any Linux host with `qemu-system-x86_64` and OVMF (`edk2-ovmf`; the firmware files are `/usr/share/OVMF/OVMF_CODE.fd` and `OVMF_VARS.fd` on Fedora):**
```
cp /usr/share/OVMF/OVMF_VARS.fd /tmp/scarfeeos-vars.fd
qemu-system-x86_64 -enable-kvm -m 4096 -smp 2 -vga virtio -display gtk \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \
  -drive if=pflash,format=raw,file=/tmp/scarfeeos-vars.fd \
  -cdrom ScarfeeOS.x86_64-0.0.1.iso -boot d
```
- [ ] `sha256sum -c` on the `.sha256` file passes.
- [ ] The GRUB menu appears; the entry name says ScarfeeOS, not Fedora.
- [ ] The kernel starts and the live system reaches a login prompt or a session without dropping to a dracut emergency shell.
- [ ] The text on screen, including any Arabic, is drawn (no empty boxes) and connected right to left. (Arabic fonts are in the image; whether the console shows them is not known.)
- [ ] `cat /etc/os-release` says ScarfeeOS; `cat /etc/yum.repos.d/*.repo` has no Cisco/OpenH264 repository.
- [ ] Network comes up (QEMU's default user network is enough: `nmcli general status`).
- [ ] Note the wall time to the login prompt, and `df -h` and `free -m` inside the live system.
- [ ] Try again with the UEFI `-drive` lines replaced by nothing (plain BIOS boot), because the BIOS side of the ISO was read in the source but not run.
- [ ] If there is no KVM, drop `-enable-kvm`; it will be slow.

**Real laptop (USB stick):** `sudo dd if=ScarfeeOS.x86_64-0.0.1.iso of=/dev/sdX bs=4M status=progress conv=fsync` (check `lsblk` first; `dd` overwrites the disk you name). Use a laptop you can afford to lose time on, and do not install anything yet.
- [ ] Secure Boot **on**: does it boot? (The shim in the image is Fedora's, signed by Microsoft; whether Fedora's keys accept ScarfeeOS's kernel and GRUB, which come from Fedora packages, is expected to work but unproven. If it fails, turn Secure Boot off and note it.)
- [ ] Secure Boot off: does it boot?
- [ ] Wi-Fi: is the adapter found, can it join a network (`NetworkManager-wifi` and `linux-firmware` are in the image)?
- [ ] Graphics at the screen's native resolution; sound output; keyboard, touchpad, brightness keys; suspend and resume.
- [ ] Bluetooth is found (`bluetoothctl list`).
- [ ] Arabic text displays correctly and an Arabic keyboard layout can be selected (the input-method wiring is not decided; see "Desktop session").
- [ ] Record laptop model, CPU, RAM, and Wi-Fi and graphics chip.

## What the image contains
- **Fonts** (docs/i18n-and-arabic.md rule 5): Noto Naskh Arabic, Noto Kufi Arabic, IBM Plex Sans Arabic (all three font packages exist in Fedora 44 under these names, licence OFL-1.1), plus Noto Sans for Latin text. HarfBuzz and FriBidi for shaping and bidi.
- **Arabic input:** `ibus` with `ibus-m17n` and `ibus-typing-booster`. The plain Arabic keyboard layout comes from the system keyboard data, not from ibus. `hunspell-ar` for spelling.
- **Locales:** `en_US` and `ar_SA` are both installed. Which one is the default is decided by the installer's language step.
- **Media:** only royalty-free codec libraries (AV1, VP9, Opus, Vorbis, FLAC, Ogg). No H.264 or H.265 packages are installed, and they are on the `<ignore>` list so they are not pulled in as dependencies. No `mpv` or `ffmpeg-free` yet, and `ffmpeg-free`, `libavcodec-free` and `gstreamer1-plugin-libav` are on the `<ignore>` list: whether Fedora's build includes patent-licensed decoders was not checked. The names that Fedora does not carry (`ffmpeg`, `x264`, `x265`, `openh264`, `libavcodec-freeworld` and others live in RPM Fusion or Cisco's repository) are kept on the ignore list as a guard.
- **Base:** kernel, systemd, NetworkManager, Bluetooth, PipeWire, power-profiles-daemon, Mesa with Vulkan (for llama.cpp on the Full tier).
- **Desktop session:** the `wayfire` Wayland compositor (it manages windows) and `webkit2gtk4.1` for the Tauri shell (see "Desktop session").

## Branded packages (docs/ip-and-licensing.md rule 6)
These carry Fedora names or logos and are not installed. Replacements:

| Fedora package | Used for | Replacement |
|----------------|----------|-------------|
| `fedora-release`, `fedora-release-common`, `fedora-release-identity-*` | `/etc/os-release`, issue files, product name | `scarfeeos-release` (`scarfeeos-release/`, Apache-2.0). `generic-release` was checked and rejected (below) |
| `fedora-logos`, `fedora-logos-classic`, `fedora-logos-httpd` | Logos, boot splash, installer and desktop branding | `scarfeeos-logos` (`scarfeeos-logos/`): one original SVG mark only. `generic-logos` was checked and rejected (below). No boot splash, installer or wallpaper artwork yet |
| Fedora wallpaper packages (`fNN-backgrounds-*`) | Wallpapers | Not installed; original wallpaper needed |
| `fedora-bookmarks`, `fedora-chromium-config` | Browser defaults | Not installed |
| `fedora-repos`, `fedora-repos-rawhide`, `fedora-repos-archive` | Repository definitions (`fedora-repos` enables a Cisco H.264 repository by default) | `scarfeeos-repos` (`scarfeeos-repos/`, Apache-2.0). `fedora-gpg-keys` is kept for signature checking |
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
- **Dependencies on Fedora packages that remain:** none by name. It requires `scarfeeos-repos(44)`, which requires `fedora-gpg-keys` (MIT) for the signing keys. The image still needs those keys: packages from Fedora are signed with Fedora's keys, so `fedora-gpg-keys` stays unless ScarfeeOS builds and signs every package itself.

### `scarfeeos-repos`
Built from `scarfeeos-repos/scarfeeos-repos.spec` (Apache-2.0). It ships one file, `/etc/yum.repos.d/scarfeeos.repo`, with two repositories: `scarfeeos-base` and `scarfeeos-updates` (names "ScarfeeOS 44 - x86_64" and "... - Updates"). Both point at Fedora's public mirror metalinks, have `gpgcheck=1`, and use the keys that `fedora-gpg-keys` installs (`/etc/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch`, a path present in the 44-2 file list). Compared with `fedora-repos`:
- **No Cisco repository.** `fedora-repos` 44-2 ships `fedora-cisco-openh264.repo` with `[fedora-cisco-openh264]` **enabled** (`skip_if_unavailable=True`), serving Cisco's patent-licensed OpenH264 binary. `scarfeeos-repos` defines no repository for it, no debug or source repositories, and no testing or rawhide repositories. `docs/ip-and-licensing.md` rule 4 is therefore met by the repository definitions; the image still contains `noopenh264`, which is a stub (see "Still open").
- **No `countme=1`.** Fedora's files set it, which makes dnf send an anonymous request counter; rule 8 says the system reports nothing by default.
- **Own repo IDs and names.** The mirror URLs and the key file names still say Fedora; that cannot change without ScarfeeOS hosting its own mirrors and signing its own packages.
- **`Conflicts: fedora-repos`**, so `fedora-repos` (and with it the Cisco repository) cannot be installed next to it by accident. Anything that hard-requires `fedora-repos` becomes uninstallable on purpose.
- The ScarfeeOS-built packages (`scarfeeos-release`, `-logos`, `-repos`) are not signed and no repository for them exists yet, so there is no `scarfeeos` repository in the file. The kiwi build reads them from `os-image/repo/`.

**Can `scarfeeos-release` drop its `fedora-repos` requirement? Yes, checked against the Fedora 44 metadata.** Method: base (`releases/44`) and updates (`updates/44`) `primary.xml.zst`, merged with the updates version winning (69,928 x86_64 and noarch packages), `scarfeeos-release` and `scarfeeos-logos` added by hand, then the `Requires:` of the install set followed (a closure over unique providers, not a real dependency solve; no dnf or libsolv was available here).
- `fedora-repos` 44-2 requires `fedora-gpg-keys >= 44` and `system-release(44)`. Only six packages in all of Fedora 44 require `fedora-repos`: `clean-rpm-gpg-pubkey`, `fedora-release-common`, `fedora-repos-archive`, `fedora-repos-rawhide`, `fedora-workstation-repositories` and `generic-release-common`. None of them is in the install set (398 packages before the desktop session, 425 after it, 426 with `scarfeeos-repos` counted), and `fedora-repos` is not either; `fedora-gpg-keys` enters only through `scarfeeos-repos`. Only `fedora-repos-eln` requires `fedora-gpg-keys`, also not in the set. No package requires a file under `/etc/yum.repos.d` or `/etc/pki/rpm-gpg`.
- Limits: 26 dependencies with several possible providers were skipped for the final set (none of the candidates is one of the packages above); boolean (rich) dependencies were not evaluated (none in the set mentions `fedora`). **No package hard-requires `fedora-repos` in this image, and the libsolv solve of 2026-10-31 (see "What was proved") agrees: it resolved with `scarfeeos-release`, `scarfeeos-repos` and no `fedora-repos`. A real `dnf` install has not been run.** Anyone later installing an RPM that needs `fedora-release-common` (for example from the packages above) will be blocked by the conflicts in `scarfeeos-release`.
- The three specs build with rpmbuild 4.18.2 (Ubuntu's, run in user space); the resulting `scarfeeos-release` requires `scarfeeos-repos(44)` and `scarfeeos-repos` provides it. Whether rpm on Fedora 44 accepts them the same way was not checked.

Still open, for legal review:
- `fedora-gpg-keys` is needed for updates, and the Fedora name appears in the repository URLs and key file names (also the `<repository>` URLs in `config.xml`). Whether the image can ship updates from Fedora's servers, and how, needs the Fedora trademark review already listed in docs/ip-and-licensing.md.
- `scarfeeos-release` declares `Conflicts: fedora-release` (as `generic-release` did), and a Requires-closure check over the repository metadata (unique providers only, not a real depsolve) found no package in the install set that needs `fedora-release`. The libsolv solve of 2026-10-31 resolved with no `fedora-release`; a real install has not been run.
- **AAC:** `pipewire-libs` (pulled in by `wireplumber`) requires `libfdk-aac.so.2`, so `fdk-aac-free` (licence `FDK-AAC`) cannot be excluded without removing PipeWire. It was therefore taken off the `<ignore>` list, which would otherwise make the build fail to resolve. It is a library dependency of the Bluetooth audio code, not something ScarfeeOS-owned code uses, but rule 4 says AAC only after the patent status is verified: **decision needed from the maintainer and legal review** (accept it, or build PipeWire without it).
- **H.264 stub:** `libheif` (pulled in through GTK 4 via `ibus-m17n`) requires `libopenh264.so.8`, satisfied by Fedora's `noopenh264` 2.6.0-4.fc44 (licence `BSD-2-Clause and LGPL-2.1-or-later`), a stub library with no codec in it. It is not the Cisco OpenH264 binary. Not removed; noted here so nobody is surprised.
- The repositories in `config.xml` (used only while building) set no `gpgcheck` options, and `scarfeeos-repos` only covers the installed system. kiwi 11.1.1 uses build repositories without signature checks (read in its source), so the packages downloaded during the build are not verified. Decide whether that is acceptable.
- The bootloader and shim may still use `fedora` as the EFI directory or boot ID. Not checked.
- No upstream package, kernel or licence notices review has been done.

## Desktop session
**Choice: `wayfire` 0.10.1 (MIT), a Wayland compositor that manages windows (floating, stacked, with title bars, snapping and switching), plus `webkit2gtk4.1` 2.54.2 (the web view that Tauri 2 uses on Linux; `shell/src-tauri/Cargo.toml` depends on `tauri` 2 with `wry`).** It replaces `cage`, the single-window kiosk compositor that the image planned before, which cannot show the side-by-side windows and taskbar in `../docs/design.md`. No display manager, no GNOME or KDE session; the panel and taskbar are the ScarfeeOS shell, not a package.

What was checked on 2026-10-31, in the Fedora 44 `primary.xml.zst` of the base and updates repositories (downloaded again), and in the `sway` 1.11-3 and `wayfire` 0.10.1-1 RPMs (downloaded and searched for strings; nothing was run):
- **Requirements for hosting a Tauri taskbar and panel.** (1) Windows the user can place and switch between. (2) A way for a client to be a panel that reserves screen space: the `wlr-layer-shell` protocol. (3) A way for the taskbar to list other programs' windows: `wlr-foreign-toplevel-management`. The `wayfire` binary contains `wlr_layer_shell_v1_create` and `wlr_xdg_decoration_manager_v1_create`, and its `foreign-toplevel` plugin (`libforeign-toplevel.so`) contains `wlr_foreign_toplevel_manager_v1_create`. `sway` contains all three as well. A string in a binary shows the code is linked in, not that it works.
- **Not tried:** a Tauri window as a layer-shell panel. Tauri 2 draws with GTK 3 on Linux, so the shell would set that up with `gtk-layer-shell` 0.10.0 (`LGPL-3.0-or-later and MIT`, linked dynamically, rule 2). It is **not** in the package list: the shell package that needs it does not exist yet, and its RPM will require it. The taskbar's window list needs the shell's Rust code to speak `wlr-foreign-toplevel-management`; that is not written.

Options compared (versions and licences from the metadata; "Added packages" from a Requires closure that follows unique providers only, counted against the other 405 packages the closure finds for the rest of `packages.txt` (without the compositor and the ScarfeeOS packages), not a real depsolve; for comparison `cage` added 11):
| | `wayfire` 0.10.1 (chosen) | `sway` 1.11 | `labwc` 0.9.6 |
|---|---|---|---|
| Licence | MIT | MIT | GPL-2.0-only (allowed as a separate program, but rule 2 prefers permissive) |
| Window model | Floating and stacking; `grid` plugin snaps windows side by side; `decoration`, `place`, `move`, `resize` plugins | Tiling (i3-style); floating needs configuration rules | Floating and stacking (openbox style) |
| Added packages | 13, of which `wayfire`, `wf-config`, `wlroots0.19` and `yyjson` are specific to it (`cage` and `wlroots` drop out) | 24 with `sway-config-upstream`, 83 with `sway-config-fedora`, because `sway` requires a `sway-config` package: the Fedora one is Fedora-branded and pulls Qt 6, `waybar` and `swaylock`, the upstream one pulls `xorg-x11-server-Xwayland` and `sway-wallpapers` | 20, including `xdg-desktop-portal` and `xdg-desktop-portal-wlr` |
| Config package needed | No; a ScarfeeOS-owned `wayfire.ini` can be added later | Yes (above) | No |
| Layer shell, toplevel list | Yes (see above) | Yes | Not checked |

Why `wayfire`:
- **Fits the concept.** The concept shows overlapping windows with a taskbar, which is a floating desktop. `sway` can be made floating by default, but its model is tiling, and its required `sway-config` package brings in `Xwayland`, which this image deliberately leaves out (below). `labwc` is also floating, but it is the only option under GPL and adds the most packages.
- **Small and permissive.** The four packages specific to it, `wayfire`, `wf-config`, `wlroots0.19` and `yyjson`, are all MIT. The `wayfire` RPM ships a sample config at `/usr/share/doc/wayfire/wayfire.ini` that enables plugins such as `cube`, `expo` and `fisheye`; the ScarfeeOS config should enable only what the design needs. That config is not written.
- **Risk, stated plainly:** `wayfire` has a much smaller user base than `sway`. If it proves unstable with the Tauri shell, `sway` with `sway-config-upstream` is the fallback, at the cost of tiling and `Xwayland`. The design's rounded corners and soft shadows (design.md, "Visual language") are not in Wayfire's core plugins as far as the metadata shows (`wayfire-plugins-extra` is not in Fedora 44); how the look will be done is undecided.
- **Licences of the rest of the session, from the Fedora 44 metadata (SPDX strings in the repository, not an independent audit).** `wlroots0.19`, `libinput`, `libseat`, `libevdev`, `mtdev`, `libliftoff`, `libwayland-server`, `libxslt` and `flite` are MIT; `libwacom` is HPND; the `xcb-util-*` packages are X11 style; `libyuv` is BSD-3-Clause; `libavif` is BSD-2-Clause, IJG and Apache-2.0 (an AV1 image library, not libav/ffmpeg); `libglvnd-gles` is MIT-style with a GPL-3.0-or-later build-script exception; `libmanette` and `xdg-dbus-proxy` are LGPL-2.1-or-later (separate library and program); `libsecret` is LGPL-2.1-or-later plus others; `harfbuzz-icu` is MIT-style; `hidapi` and `hyphen` are multi-licensed with a permissive choice (BSD-3-Clause, MPL-1.1/LGPL); `libatomic` is GPL-3.0-or-later with the GCC runtime exception. `webkit2gtk4.1` and `javascriptcoregtk4.1` carry a long list led by LGPL-2.1-only; the Tauri shell links them dynamically, which rule 2 allows. Rule 2 also asks for an SBOM; none exists yet.
- **Codecs:** none of the packages added matches the patent-codec names in `packages.txt` (`openh264`, `x264`, `x265`, `ffmpeg*`, `libavcodec*`, `gstreamer1-plugin-libav`, `gstreamer1-plugins-bad-freeworld`). `webkit2gtk4.1` recommends GStreamer plugins, but kiwi installs only required dependencies by default (`patternType` is not set in `config.xml`, so it is `onlyRequired`; checked in kiwi's `xml_state.py`), so the embedded browser has no H.264 decoder, which matches rule 5.
- **Wayland native.** GTK 3 and WebKitGTK run on Wayland directly; `Xwayland` is not installed (`wayfire` does not require it), so X11-only programs will not start. Arabic input goes through the `ibus` packages already in the image; `wayfire` has an `input-method-v1` plugin, but how ibus is started inside the session is not decided.
- **Who runs it:** `systemd-logind` (already in `systemd`) gives `wayfire` its seat through `libseat`, so no `seatd` is needed. A systemd unit that logs a user in on `tty1` and starts `wayfire` with the shell (or the installer front-end) is **not written**: it needs the user accounts that the installer creates and the shell does not exist as a package yet.

Other options checked in the same metadata and not chosen: `gdm` 50.3, `sddm` 0.21.0, `mutter` 50.6 and `gnome-kiosk` 50.1 (GPL-2.0-or-later; a GNOME session is a large stack with its own branding and settings); `weston` 15.0.1 (`MIT and CC-BY-SA-3.0`, a reference compositor); `niri` 26.04 and `river` 0.5.0 (GPL-3.0); `miriway` 26.01 (GPL-3.0-only); `greetd` 0.10.3 (GPL-3.0-only; a login daemon is not needed yet). `hyprland` is not in Fedora 44.

**Not done:** no `wayfire` session has been started, no Tauri window has been run under it, and no layer-shell panel or taskbar exists. The choice rests on package metadata and strings in the binaries.

## Language selection comes first
docs/installer.md requires the very first installer screen to be the bilingual "Select your language / اختر لغتك" screen, with nothing else possible before it. **None of this is built.** The plan:

1. The ISO boots to the Wayland session (`wayfire`, see "Desktop session") that starts only the ScarfeeOS installer front-end, full screen. Both locales, all the fonts above and ibus are already on the image, so the first screen renders correctly before any choice.
2. The front-end's first screen shows the two buttons, English and العربية. Its strings go through Fluent like the rest of the project (`locales/`).
3. The choice is written to a small answers file. It sets the system locale, the keyboard layout (with the other language's layout added as an option), the default AI conversation and speech language, and the calendar and numeral defaults, as listed in docs/installer.md. The rest of the installer then switches to that language, with RTL layout for Arabic.
4. Later steps (disk and account, hardware check and AI tier, optional Claude key, finish) follow, and the actual installation is done by existing Fedora tooling driven by the answers file.

Not decided yet: whether the install back-end is Anaconda (using a kickstart generated from the answers file) or something smaller. Anaconda's own first screen is a language list, but it is Fedora-branded and is not the two-button screen in docs/installer.md, so it is not a substitute.

## Not built yet
- Any image (this definition has been validated against kiwi 11.1.1's schema and dependency-solved, but never built; see "Status").
- Installer front-end, the language screen, answers file, and the install back-end.
- Desktop session: `wayfire` and `webkit2gtk4.1` are in the package list, but no unit starts them, no login or autologin is defined, and no ScarfeeOS shell package exists. Never run.
- Wallpapers, boot splash and installer artwork (only the one SVG logo exists).
- Signing of the ScarfeeOS packages, and a repository to serve them.
- A real `dnf5` install of the set in a chroot (the metadata-level solve is done; see "What was proved").
- Root and user accounts (the installer's account step creates them).
- A system keyring package for the Claude API key (not chosen).
- Tier-specific content: model files, llama.cpp and speech engines are not in the image.
- SBOM and licence check for the image (rule 2).
