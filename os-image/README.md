# os-image

Fedora-based image build and installer. The installer's first screen is language selection (English / العربية); see ../docs/installer.md.

## Status: first draft, nothing built
- **No image has been built.** kiwi was not available when this was written.
- **Package names were not checked against a live Fedora repository.** They were written from memory and may be wrong, renamed, or missing in the chosen release. The kiwi attributes in `config.xml` (schema version, `dnf4`, ISO type options, `<ignore>`) were also not validated against kiwi.
- The only automated check is `test_image_definition.py`: it parses `config.xml` with Python's standard library (well-formedness only, not the kiwi schema) and checks the policy rules below.

## Files
| File | Purpose |
|------|---------|
| `config.xml` | kiwi image description: x86_64 ISO, UEFI and BIOS boot, English and Arabic locales |
| `packages.txt` | Readable package list; must match `config.xml` (the test checks this) |
| `test_image_definition.py` | Well-formedness and policy checks |

## Build steps (untested)
Needs a Fedora machine with root access.

1. Install kiwi: `sudo dnf install kiwi-cli` (package name not verified).
2. Edit `config.xml`: confirm the Fedora release in the two `<repository>` paths is still supported, and set a real `<contact>`.
3. Build: `sudo kiwi-ng --type iso system build --description os-image --target-dir /tmp/scarfeeos-build`
4. The ISO is written to the target directory. Boot it in a VM before anything else.
5. Check: `python3 os-image/test_image_definition.py` (this works without kiwi).

Expect the first real build to fail and need fixes to package names and kiwi options.

## What the image contains
- **Fonts** (docs/i18n-and-arabic.md rule 5): Noto Naskh Arabic, Noto Kufi Arabic, IBM Plex Sans Arabic (`ibm-plex-sans-arabic-fonts` is the least certain name), plus Noto Sans for Latin text. HarfBuzz and FriBidi for shaping and bidi.
- **Arabic input:** `ibus` with `ibus-m17n` and `ibus-typing-booster`. The plain Arabic keyboard layout comes from the system keyboard data, not from ibus. `hunspell-ar` for spelling.
- **Locales:** `en_US` and `ar_SA` are both installed. Which one is the default is decided by the installer's language step.
- **Media:** only royalty-free codec libraries (AV1, VP9, Opus, Vorbis, FLAC, Ogg). No H.264 or H.265 packages are installed, and they are on the `<ignore>` list so they are not pulled in as dependencies. AAC (`fdk-aac-free`) is also excluded until its patent status is verified (rule 4). No `mpv` or `ffmpeg-free` yet: whether Fedora's build includes patent-licensed decoders was not checked.
- **Base:** kernel, systemd, NetworkManager, Bluetooth, PipeWire, power-profiles-daemon, Mesa with Vulkan (for llama.cpp on the Full tier).

## Branded packages (docs/ip-and-licensing.md rule 6)
These carry Fedora names or logos and are not installed. Replacements:

| Fedora package | Used for | Replacement |
|----------------|----------|-------------|
| `fedora-release`, `fedora-release-common`, `fedora-release-identity-*` | `/etc/os-release`, issue files, product name | `generic-release` for now; a `scarfeeos-release` package is **not built yet** |
| `fedora-logos`, `fedora-logos-classic`, `fedora-logos-httpd` | Logos, boot splash, installer and desktop branding | `generic-logos` for now; original ScarfeeOS artwork is **not made yet** |
| Fedora wallpaper packages (`fNN-backgrounds-*`) | Wallpapers | Not installed; original wallpaper needed |
| `fedora-bookmarks`, `fedora-chromium-config` | Browser defaults | Not installed |
| `fedora-flathub-remote`, `fedora-third-party`, `fedora-workstation-repositories` | Third-party repositories | Not installed |

Still open, for legal review:
- `fedora-repos` and `fedora-gpg-keys` are normally needed for updates, and the Fedora name appears in repository definitions and URLs. The `<repository>` URLs in `config.xml` name Fedora. Whether the image can ship updates from Fedora's servers, and how, needs the Fedora trademark review already listed in docs/ip-and-licensing.md.
- Whether `generic-release` can replace `fedora-release` without dependency conflicts has not been tested.
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
- Any image (this definition has never been run through kiwi).
- Installer front-end, the language screen, answers file, and the install back-end.
- Desktop session: the shell (`../shell/`) does not exist yet, so no compositor or desktop is in the package list and the live image has no graphical session or login defined.
- `scarfeeos-release`, ScarfeeOS logos and wallpapers.
- Root and user accounts (the installer's account step creates them).
- A system keyring package for the Claude API key (not chosen).
- Tier-specific content: model files, llama.cpp and speech engines are not in the image.
- SBOM and licence check for the image (rule 2).
