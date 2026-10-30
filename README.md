# AIOS

An AI-first operating system for laptops. Talk to it or type to it, in **English or Arabic**.

> Status: early design and scaffolding. Nothing here is usable yet.

## Vision

- **AI voice and text agent** built into the system. It opens apps, manages files, schedules meetings and helps with tasks.
- **Bilingual from the first boot:** English and Arabic are both main languages. The installer asks which one to use before anything else, and the language can be switched later without a restart. Arabic is fully right-to-left (RTL) across the shell, apps, speech and the AI.
- **Smooth desktop**, familiar to Windows and Fedora users.
- **Built-in apps:** documents, spreadsheets, presentations, email and calendar, a media player, a meetings and scheduling manager, and a web browser.
- **Runs on business laptops** (Ryzen or Intel, with or without a GPU). Local-first AI, with an optional Claude connection for users who want it.

## Approach

AIOS is a Linux-based system (Fedora as the base), not a new kernel. See [docs/architecture.md](docs/architecture.md).

## Repository layout

| Path | Purpose |
|------|---------|
| `core/` | Rust AI service: model providers, speech, task/tool API |
| `shell/` | Desktop shell (Tauri UI, Wayland compositor integration) |
| `apps/` | Productivity apps, media player, browser, meetings manager |
| `os-image/` | Fedora-based image build and the installer |
| `locales/` | Translations (`en`, `ar`) in Fluent format |
| `docs/` | Architecture, installer and Arabic/i18n design rules |

## Languages

English and Arabic are required for every user-facing feature. See [docs/i18n-and-arabic.md](docs/i18n-and-arabic.md).

## Roadmap

See [ROADMAP.md](ROADMAP.md).
