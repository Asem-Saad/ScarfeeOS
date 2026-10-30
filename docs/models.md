# Models and datasets

Candidates for the local AI. Policy: Apache-2.0 or MIT only, no custom or use-restricted terms (see [ip-and-licensing.md](ip-and-licensing.md)). A model is only a default after it passes the Arabic and English benchmark in `core/eval/`.

Status: **verified** = license read on the official model card or repository on 2026-10-30. **unverified** = reported by secondary sources only; do not rely on it.

## Text (LLM)

| Model | Size | License | Status | Notes |
|-------|------|---------|--------|-------|
| Qwen3-4B | 4.0B | Apache-2.0 | verified (HF card) | Card says 100+ languages but does not name Arabic. Arabic quality must come from our benchmark. |
| Qwen3-1.7B | 1.7B | Apache-2.0 | unverified | Low-RAM tier candidate. |
| Qwen3-8B | 8B | Apache-2.0 | unverified | 16 GB+ tier candidate. |
| Qwen3-0.6B / 14B / 32B | - | Apache-2.0 | unverified | Check each size individually; a family can mix licenses. |

Community Arabic fine-tunes of Qwen3 (for example on featherless.ai) are not used until their training data and license are checked.

## Speech to text

| Model | Size | License | Status | Notes |
|-------|------|---------|--------|-------|
| Whisper (OpenAI) | tiny 39M, base 74M, small 244M, medium 769M, turbo 809M | MIT (code and weights) | verified (GitHub README) | VRAM: small ~2 GB, medium ~5 GB, turbo ~6 GB. Training data is not disclosed by OpenAI. |
| Cohere Transcribe Arabic | - | Apache-2.0 (reported) | unverified | Arabic-specific; worth benchmarking. |

## Text to speech

| Model | Size | License | Status | Notes |
|-------|------|---------|--------|-------|
| SILMA TTS v1 | 150M | Code MIT, weights Apache-2.0 | verified (GitHub README) | Arabic (MSA) and English. Training data described only as "public and proprietary"; datasets and their licenses are not named. Repo disclaimer asks for consent when cloning voices. The README is inconsistent (says "Apache 2.0" overall), so confirm the weights license in the HF repo. |
| Habibi-TTS | - | Code MIT; per-checkpoint weights | unverified | ALG, EGY, IRQ, MAR, MSA checkpoints reported Apache-2.0. Unified, SAU, UAE checkpoints are CC-BY-NC-SA-4.0 and excluded. |

Excluded: Arabic-TTS-Spark (no commercial use), nipponjo/tts_arabic (unclear license, XTTS-derived voices), phoonnx Miro (CC BY-NC-ND), XTTS (non-commercial).

## Training data

| Dataset | License | Status |
|---------|---------|--------|
| Mozilla Common Voice (Arabic) | CC0 (expected) | unverified. Since October 2025 it is distributed through Mozilla Data Collective; the license must be read there before use. |
| Own recordings | Owned by the project | Planned for the assistant voice (about 2 to 5 hours, Arabic and English). |

## Open items
- Verify the Qwen3 sizes beyond 4B and Qwen3 Arabic quality.
- Confirm SILMA weights license on Hugging Face and ask the authors about training data.
- Confirm the Common Voice license at Mozilla Data Collective.
- Verify Cohere Transcribe Arabic and Habibi-TTS checkpoint licenses.
- Record the training-data statement for each model once the default is chosen.
