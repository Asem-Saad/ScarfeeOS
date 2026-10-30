# Architecture

## Base
A custom Fedora spin (built with kiwi or lorax). Fedora provides the kernel, drivers, networking and power management. AIOS adds the shell, AI core and apps.

## Components
```
 voice / text input
        |
   [ shell UI (Tauri) ] <---> [ core service (Rust) ]
                                   |-- speech: STT, TTS, wake word
                                   |-- router --> local provider (llama.cpp)
                                   |          \-> claude provider (optional)
                                   '-- tool API --> apps, files, calendar
```

## AI core
- **Local provider:** quantized GGUF models via llama.cpp. Tier chosen at first run from detected RAM/CPU (about 3B at 8 GB, about 7-8B at 16 GB or more). Uses AVX2/AVX-512, Vulkan or OpenVINO where available.
- **Claude provider (optional):** API key stored in the system keyring. A Claude.ai subscription is not API access, so sign-in is key-based unless Anthropic's terms allow otherwise.
- **Router:** simple commands stay local; heavy tasks go to Claude only if configured and allowed. "Local only" mode disables all network providers.
- **Speech:** whisper.cpp for STT, a TTS engine with Arabic voices, an Arabic-aware wake word. All offline.

## Apps
Reuse mature engines where possible (LibreOffice for office formats, mpv or libmpv as a separate program for media, a WebView first and later Gecko or Chromium (without proprietary codecs) for the browser). Meetings and scheduling are original, built on CalDAV/ICS.
