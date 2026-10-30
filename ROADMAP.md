# Roadmap

Stages, status and the current agenda are in [docs/lifecycle.md](docs/lifecycle.md). A box is ticked only when the work exists in the repository. Where real-hardware testing is still needed, the item says so.

## Phase 0: Foundations
- [x] Repo structure and design docs
- [x] Project license: Apache-2.0
- [x] Benchmark candidate local models (only commercially permissive licenses; see docs/ip-and-licensing.md and docs/models.md) on Arabic and English tasks
- [ ] Complete legal review (see docs/ip-and-licensing.md), kept until a working product exists

## Phase 1: AI core (runs on any Linux machine)
- [x] Local provider over an OpenAI-compatible API (llama.cpp or vLLM)
- [ ] Claude provider (API key)
- [x] Hardware detection and AI tier selection
- [x] Task/tool API (files, apps, reminders, calendar), with opening desktop apps still untested for real
- [x] Exact logic in code: relative dates, clock, approximate Hijri dates
- [ ] Speech to text: built on whisper.cpp, needs testing on real Arabic speech and a real microphone
- [ ] Text to speech: interface built, needs a voice that meets the licensing policy
- [ ] Wake word
- [ ] Arabic dialect evaluation for speech
- [ ] "Local only" privacy mode as a user setting

## Phase 2: Shell and first apps
- [x] Assistant panel with full RTL mirroring and runtime language switch
- [x] Meetings screen with an ICS calendar store and approximate Hijri dates
- [ ] CalDAV sync
- [ ] Desktop shell: Tauri build exists, its window is still untested
- [ ] Installer screens working in a browser
- [ ] Voice in the assistant panel

## Phase 3: Productivity suite
- [ ] Documents, spreadsheets, presentations (RTL, Arabic numerals)
- [ ] Email and calendar client
- [ ] Media player (royalty-free codecs bundled; patent-licensed codecs via user add-on)
- [ ] Browser (WebView first, no proprietary codecs; full Gecko/Chromium build later)

## Phase 4: Bootable image
- [ ] Fedora-based image build: definition and release packages exist, no image built yet
- [ ] Installer with language selection first (see docs/installer.md)
- [ ] Hardware testing on business laptops (Ryzen and Intel)
