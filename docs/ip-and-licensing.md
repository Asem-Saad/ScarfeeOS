# IP and licensing policy

Goal: keep legal and IP risk as low as practical. No policy can guarantee zero risk, especially for patents. A qualified lawyer must review the project before any release.

## Rules
1. **Original code only.** No code, assets, icons, fonts, sounds or text copied from proprietary or unclear-license sources. No copying from leaked or decompiled software.
2. **Permissive licenses only for dependencies:** MIT, Apache-2.0, BSD, ISC, MPL-2.0 (file-level copyleft), and LGPL only when dynamically linked. **No GPL/AGPL in code we ship as part of AIOS-owned components** without explicit review. Every dependency is recorded with its license (SBOM) and checked in CI (e.g. `cargo deny`).
3. **AI models and voices:** only models whose license permits commercial use and redistribution without custom restrictions (Apache-2.0, MIT). Models under custom or restricted terms (e.g. Llama, Gemma, non-commercial voices like XTTS) are **excluded**. Each model's license and training-data statement is recorded in `docs/models.md`.
4. **Patent-encumbered formats:** do not ship or bundle codecs or formats that require patent licensing (e.g. H.264/H.265, AAC, MP3 where applicable) unless a license is obtained. Prefer royalty-free formats: AV1, VP9, Opus, Vorbis, FLAC, WebM, Ogg, Matroska, ODF/OOXML via open implementations.
5. **No trademark use.** Do not use the names, logos or trade dress of Windows, Microsoft Office (Word, Excel, PowerPoint, Outlook), Fedora, VLC, Chrome or others. AIOS apps get their own names and original visual design. "Familiar" means standard UX conventions, not copied designs.
6. **Third-party services:** use Claude only through its public API under Anthropic's terms, optional and off by default. Users supply their own key.
7. **Own license:** the project picks an explicit open license (to be decided; Apache-2.0 includes a patent grant) and every file carries attribution where required.
8. **Contributions:** contributors certify they have the right to submit their work (DCO sign-off).

## Open items
- Choose the project license.
- Legal review: patents (freedom-to-operate), trademarks, and base-distribution terms (Fedora/Linux kernel are GPL; shipping them has obligations).
- Create `docs/models.md` and the dependency license check in CI.
