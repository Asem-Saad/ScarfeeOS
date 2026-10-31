# Project lifecycle and agenda

How ScarfeeOS goes from an idea to a released, maintained operating system, where it stands today, and what comes next. Stages run in order, but work can overlap. There are no target dates yet. They will be added once a bootable image exists and the remaining work can be measured.

Task-level detail is in [ROADMAP.md](../ROADMAP.md). How it will look is in [design.md](design.md). Research for stage 6 is in [productivity-apps.md](productivity-apps.md).

## Stages

| # | Stage | Goal | Done when | Status |
|---|-------|------|-----------|--------|
| 1 | Foundations | Set the direction and rules | Design docs, licensing policy and benchmark set exist | Done |
| 2 | AI core | An assistant that understands requests and acts on them correctly | English and Arabic requests run correctly through the core, with exact results computed in code | Mostly done |
| 3 | Speech | Talk to the system and hear it reply | Arabic and English work by voice on real hardware, using a voice whose license meets the policy | In progress |
| 4 | Desktop shell and first apps | A usable bilingual interface | The Tauri shell runs on Linux, with the assistant, settings and meetings screens working | In progress |
| 5 | Bootable image | ScarfeeOS installs and starts | An image is built, boots on a real laptop and completes the installer | In progress |
| 6 | Productivity apps | Everyday work | Documents, spreadsheets, presentations, email, media and browser work in both languages | Not started |
| 7 | Hardware and alpha testing | It works outside the development machine | The AI tiers are measured on Ryzen and Intel business laptops, and native Arabic speakers have tested it | Not started |
| 8 | Legal review | Clear to release | A qualified lawyer has reviewed the items in [ip-and-licensing.md](ip-and-licensing.md) and they are resolved | Not started, deliberately kept until a working product exists |
| 9 | Beta and first release | People can use it | Public beta, fixes, then version 1.0 with install and update instructions | Not started |
| 10 | Maintenance | It stays safe and improves | Security updates, new Fedora bases, better models and community contributions, on a regular release cycle | Not started |

## Where things stand

**Working in development:**
- An assistant core that calls a local model, chooses tools, and computes dates, the clock and approximate Hijri dates in code. Half hours, quarter hours and the common weekday forms in Egyptian and Gulf Arabic are read in code, not by the model.
- Real actions: reminders and meetings saved to a calendar file, file search and listing, with confirmation before settings change. Reminder text and meeting titles use the user's own words.
- An assistant panel in English and Arabic with full right-to-left layout and a language switch that needs no restart, plus a meetings screen.
- The three installer screens (language, AI tier, disk placeholder) load and work in a browser in both languages. Nothing is written to a disk.
- Automatic hardware detection and three AI tiers, with Full and Dual model choices based on benchmarks on one 6 GB GPU.
- A benchmark of Arabic and English prompts, including Egyptian, Gulf and Levantine phrasing. On Qwen3-8B, on one 6 GB GPU, the 30 Egyptian and Gulf prompts gave the right tool choice 26 of 26 times and 0 leaked characters. The reply quality was not judged by a native speaker.
- Automated tests and license checks. The tests pass on Windows, which is only a development platform; the shell tests that need Unix are skipped there.

**Built but not proven on real hardware yet:**
- The desktop window, because it has only been built, not opened.
- The microphone path. Speech to text has run on real English speech, but not on real Arabic speech.
- Opening desktop applications.
- AMD and Intel graphics detection. It is written and tested against sample files, never on a real AMD or Intel machine.
- The OS image. A build script and checks of the image definition against Fedora 44 package data exist. No image has ever been built or booted.

**Known problems:**
- No Arabic voice can ship yet. The leading candidate's dependencies appear to conflict with the licensing policy, and no safe Egyptian or Gulf voice has been found. The voice is unresolved (see [voice-options.md](voice-options.md)).
- The local model has answered an English question in Arabic and can be weak on general knowledge. The reply-language and leaked-character checks reduce this but do not remove it.
- The dialect result above is one model, one GPU and 30 prompts. Other models, other dialects and other hardware are not measured.
- Productivity apps are only researched ([productivity-apps.md](productivity-apps.md)); nothing is installed or tested.

## Agenda

Current priorities, in order:

1. Resolve the voice: check whether the conflicting dependencies can be removed, and evaluate other voices. If none qualifies, record a project-owned voice.
2. Measure the Dual tier on real hardware, with Qwen for commands and Jais for Arabic writing and explanation.
3. Test real microphone capture and real Arabic speech recognition.
4. Open and test the desktop window on Linux.
5. Run the first image build as root on Fedora, then boot it in QEMU and on a real laptop.
6. Test hardware detection on real AMD and Intel machines.
7. Start the productivity apps, using [productivity-apps.md](productivity-apps.md), then broader hardware testing with native speakers.

Later, before any public release: the legal review, including patent questions, Fedora trademark and redistribution terms, and AAC audio in the image.

This page is updated as stages move. A stage is only marked done when it has been proven to work, not just written.
