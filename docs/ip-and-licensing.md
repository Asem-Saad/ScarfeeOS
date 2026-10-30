# IP and licensing policy

Goal: keep legal and IP risk as low as practical. No policy can guarantee zero risk, especially for patents. A qualified lawyer must review the project before any release.

## Project license
ScarfeeOS-owned code is licensed under **Apache-2.0** (see `LICENSE`), which includes a patent grant from contributors. Contributors sign off commits under the Developer Certificate of Origin (DCO) to certify they have the right to submit their work.

## Rules
1. **Original code only.** No code, assets, icons, fonts, sounds or text copied from proprietary or unclear-license sources, and nothing from leaked or decompiled software.
2. **Dependencies:**
   - Preferred: MIT, Apache-2.0, BSD, ISC, MPL-2.0.
   - LGPL: only dynamically linked.
   - GPL/AGPL: allowed only as separate, unmodified programs that ScarfeeOS-owned code talks to at arm's length (the Linux kernel, the Fedora base, mpv if used). ScarfeeOS-owned code never links to GPL libraries. Source offers and notices are provided where the license requires them.
   - Every dependency and its license is recorded (SBOM) and checked in CI (e.g. `cargo deny`).
3. **AI models and voices:** only models and voices under Apache-2.0 or MIT, with no custom or use-restricted terms. Excluded: Llama, Gemma, non-commercial voices (e.g. XTTS). The license is verified per model and per size (a model family can mix licenses) and recorded in `docs/models.md` with its training-data statement.
4. **Media formats and codecs:**
   - Bundled by default: royalty-free formats (AV1, VP9, Opus, Vorbis, FLAC, WebM, Ogg, Matroska) and MP3 (patents expired). AAC only after the patent status is verified.
   - H.264/H.265 and similar patent-licensed codecs are **not bundled**. The media player uses codecs already on the user's system or an optional add-on that the user installs themselves, as Fedora and other Linux distributions do.
   - Documents: open implementations of ODF and OOXML.
5. **Browser:** start with a WebView-based browser built without proprietary codecs. A full Gecko or Chromium build comes later and is also built without them.
6. **No trademark use.** Do not use the names, logos or trade dress of Windows, Microsoft Office (Word, Excel, PowerPoint, Outlook), Fedora, VLC, Chrome or others. ScarfeeOS is a distinct distribution with its own name, and its apps get their own names and original visual design. "Familiar" means standard UX conventions (taskbar, menus, file manager), not copied designs.
7. **Third-party services:** Claude is used only through its public API under Anthropic's terms, optional and off by default. Users supply their own key.
8. **Privacy:** user data stays on the device by default. Network providers only run when the user enables them. "Local only" mode disables all of them.

## Open items for legal review
- Patent freedom-to-operate for core features (e.g. the voice agent).
- Fedora trademark and redistribution terms, and GPL obligations of the base image.
- Anthropic API terms.
- Create `docs/models.md` and the CI license check.
