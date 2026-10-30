# Roadmap

## Phase 0: Foundations
- [x] Repo structure and design docs
- [x] Project license: Apache-2.0
- [ ] Complete legal review (see docs/ip-and-licensing.md)
- [ ] Benchmark candidate local models (only commercially permissive licenses, e.g. Apache-2.0 Qwen; see docs/ip-and-licensing.md) on Arabic and English tasks

## Phase 1: AI core (runs on any Linux machine)
- [ ] Provider abstraction: `local` (llama.cpp) and `claude` (API key)
- [ ] Hardware detection and automatic model tier selection
- [ ] Speech pipeline: whisper.cpp (STT), TTS, wake word, Arabic dialect evaluation
- [ ] Task/tool API (open app, files, reminders, calendar)
- [ ] "Local only" privacy mode

## Phase 2: Shell and first apps
- [ ] Desktop shell with full RTL mirroring and runtime language switch
- [ ] Meetings and scheduling manager (CalDAV/ICS, Hijri calendar)
- [ ] Voice and text assistant panel

## Phase 3: Productivity suite
- [ ] Documents, spreadsheets, presentations (RTL, Arabic numerals)
- [ ] Email and calendar client
- [ ] Media player (royalty-free codecs bundled; patent-licensed codecs via user add-on)
- [ ] Browser (WebView first, no proprietary codecs; full Gecko/Chromium build later)

## Phase 4: Bootable image
- [ ] Fedora-based image build
- [ ] Installer with language selection first (see docs/installer.md)
- [ ] Hardware testing on business laptops (Ryzen and Intel)
