# Roadmap

Stages, status and the current agenda are in [docs/lifecycle.md](docs/lifecycle.md). A box is ticked only when the work exists in the repository. Where real-hardware testing is still needed, the item says so. Research for Phase 3 is in [docs/productivity-apps.md](docs/productivity-apps.md).

## Phase 0: Foundations
- [x] Repo structure and design docs
- [x] Project license: Apache-2.0
- [x] Benchmark candidate local models (only commercially permissive licenses; see docs/ip-and-licensing.md and docs/models.md) on Arabic and English tasks
- [ ] Complete legal review (see docs/ip-and-licensing.md), kept until a working product exists

## Phase 1: AI core (runs on any Linux machine)
- [x] Local provider over an OpenAI-compatible API (llama.cpp or vLLM)
- [ ] Claude provider (API key)
- [x] Hardware detection and AI tier selection (AMD and Intel graphics are detected but untested on real hardware)
- [x] Task/tool API (files, apps, reminders, calendar), with opening desktop apps still untested for real. Reminder text and meeting titles use the user's words
- [x] Exact logic in code: relative dates, clock, half and quarter hours, common weekday forms, approximate Hijri dates
- [ ] Speech to text: built on whisper.cpp, needs testing on real Arabic speech and a real microphone
- [ ] Text to speech: interface built, needs a voice that meets the licensing policy
- [ ] Wake word
- [ ] Arabic dialect evaluation for speech
- [x] Arabic dialect evaluation for commands: 26 of 26 tool choices and 0 leaked characters on 30 Egyptian and Gulf prompts, Qwen3-8B, one 6 GB GPU. Reply quality not judged by a native speaker
- [ ] "Local only" privacy mode as a user setting

## Phase 2: Shell and first apps
- [x] Assistant panel with full RTL mirroring and runtime language switch
- [x] Meetings screen with an ICS calendar store and approximate Hijri dates
- [ ] CalDAV sync
- [ ] Desktop shell: Tauri build exists, its window is still untested
- [x] Installer screens working in a browser (language, AI tier, disk placeholder; nothing is written to a disk)
- [ ] Voice in the assistant panel

## Phase 3: Productivity suite
- [ ] Documents, spreadsheets, presentations (RTL, Arabic numerals); options researched, nothing installed or tested
- [ ] Email and calendar client
- [ ] Media player (royalty-free codecs bundled; patent-licensed codecs via user add-on)
- [ ] Browser (WebView first, no proprietary codecs; full Gecko/Chromium build later)

## Phase 4: Bootable image
- [ ] Fedora-based image build: definition, release packages, a build script and checks exist; no image has been built or booted
- [ ] Installer with language selection first (see docs/installer.md)
- [ ] Hardware testing on business laptops (Ryzen and Intel)
