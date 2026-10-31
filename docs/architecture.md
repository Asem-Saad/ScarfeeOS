# Architecture

## Base
A custom Fedora spin (built with kiwi or lorax). Fedora provides the kernel, drivers, networking and power management. ScarfeeOS adds the shell, AI core and apps.

## Components
```
 voice / text input
        |
   [ shell UI (Tauri) ] <---> [ core service (Rust) ]
                                   |-- speech: STT, TTS, wake word
                                   |-- router --> local provider (llama.cpp or vLLM)
                                   |          \-> claude provider (optional)
                                   |-- exact logic: dates, clock, calculations
                                   '-- tool API --> apps, files, calendar
```

## AI core
- **Local provider:** talks to the engine over an OpenAI-compatible local API, so the engine can be swapped. Tier chosen during installation (see [installer.md](installer.md)). The Lite tier uses quantized GGUF models via llama.cpp, with AVX2/AVX-512, Vulkan or OpenVINO where available. The Full tier uses a GPU, through llama.cpp or vLLM; the choice and the model sizes for each tier depend on benchmarks.
- **Exact logic:** anything that must be correct, such as dates and calculations, is computed in code. The model only understands the request.
- **No religious content:** ScarfeeOS does not provide religious content. The assistant declines such questions, and none are included in its knowledge or evaluation data.
- **Claude provider (optional):** API key stored in the system keyring. A Claude.ai subscription is not API access, so sign-in is key-based unless Anthropic's terms allow otherwise.
- **Router:** simple commands stay local; heavy tasks go to Claude only if configured and allowed. "Local only" mode disables all network providers.
- **Speech:** whisper.cpp for STT, a TTS engine with Arabic voices, an Arabic-aware wake word. All offline.

## Apps
Reuse mature engines where possible (LibreOffice for office formats, mpv or libmpv as a separate program for media, a WebView first and later Gecko or Chromium (without proprietary codecs) for the browser). Meetings and scheduling are original, built on CalDAV/ICS. The research behind these choices is in [productivity-apps.md](productivity-apps.md).
