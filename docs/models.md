# Models and datasets

Candidates for the local AI. Policy: Apache-2.0 or MIT only, no custom or use-restricted terms (see [ip-and-licensing.md](ip-and-licensing.md)). A model is only a default after it passes the Arabic and English benchmark in `core/eval/`. The owner's choice of models is recorded under "Benchmark on the RTX 3050" below; it rests on the owner's own measurements and judgement. The `core/eval/` dialect run on Qwen3-8B is noted in the table below.

Status: **verified** = the license text (LICENSE file) or the license statement on the official model card or repository was read on the primary source on the date shown. **unverified** = not read on a primary source, or the primary sources conflict; do not rely on it. Secondary sources were not used. "Verified" says what the license is, not that it passes rule 3 (see the Habibi rows).

"Checked" dates are 2026-10-30, except the Vocos, CATT and Jais tech-report checks, which are 2026-10-31. Counts at the end of this file.

## Text (LLM)

Qwen3 sizes. All six repositories have a `LICENSE` file; the text is the unmodified Apache License 2.0 (copyright "2024 Alibaba Cloud") and is byte-identical across sizes. Each card header also says `license: apache-2.0`. None of the cards has a training-data statement (only "Training Stage: Pretraining & Post-training"), so training data is **not disclosed** on the cards. The cards say "100+ languages and dialects" and never name Arabic; Arabic quality must come from our benchmark.

| Model | Params (card) | License | Status | Source |
|-------|---------------|---------|--------|--------|
| Qwen3-0.6B | 0.6B | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-0.6B |
| Qwen3-1.7B | 1.7B | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-1.7B |
| Qwen3-4B | 4.0B | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-4B |
| Qwen3-8B | 8.2B | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-8B |
| Qwen3-14B | 14.8B | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-14B |
| Qwen3-32B | 32.8B | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-32B |

Qwen3 GGUF repositories (for llama.cpp). Each has its own `LICENSE` file: the same Apache License 2.0 text, with copyright year "2025 Alibaba Cloud". Card header `license: apache-2.0`. Not gated. Training data: **not disclosed** on the cards.

| Repository | License | Status | Source |
|------------|---------|--------|--------|
| Qwen3-0.6B-GGUF | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-0.6B-GGUF |
| Qwen3-1.7B-GGUF | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-1.7B-GGUF |
| Qwen3-4B-GGUF | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-4B-GGUF |
| Qwen3-8B-GGUF | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-8B-GGUF |
| Qwen3-14B-GGUF | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-14B-GGUF |
| Qwen3-32B-GGUF | Apache-2.0 | verified 2026-10-30 (LICENSE) | https://huggingface.co/Qwen/Qwen3-32B-GGUF |

Further Arabic-capable candidates to benchmark at 6 GB of GPU memory or less. Whether they fit was not measured here; sizes are from the cards and file headers.

| Model | Params | License | Status | Source | Training data / notes |
|-------|--------|---------|--------|--------|-----------------------|
| Phi-4-mini-instruct (Microsoft) | 3.8B (card) | MIT | verified 2026-10-30 (LICENSE file; card says "licensed under the MIT license") | https://huggingface.co/microsoft/Phi-4-mini-instruct | Card: 5T tokens from filtered public documents and code, synthetic "textbook-like" data, and chat-format supervised data; public-data cutoff June 2024; individual datasets not named. Arabic is in the card's supported-language list, but the card also says the model is trained primarily on English and other languages perform worse, so Arabic quality is unknown until benchmarked. |
| Jais-3-3.7B (Inception42/IFM) | 5.06B (HF safetensors metadata) | Apache-2.0 | verified 2026-10-30 (card statement `license: apache-2.0`; the repo has no LICENSE file) | https://huggingface.co/inception42/Jais-3-3.7B | Arabic and English only (card). Training data **not disclosed** on the card. The card links the Jais 3 Technical Report (https://ifm.ai/jais/tech-report) and a blog post (https://ifm.ai/blog/jais3/); on 2026-10-31 both returned HTTP 403 with a Cloudflare "Just a moment" challenge page, so neither could be read and there is still no training-data statement. The transformers path needs `trust_remote_code` custom architecture code. Card also says Jais-3-0.9B (https://huggingface.co/inception42/Jais-3-0.9B) is `apache-2.0`; not counted, as only its header was read. |
| Jais-3-3.7B-GGUF (IFM) | same | Apache-2.0 | verified 2026-10-30 (card statement `license: apache-2.0`; no LICENSE file) | https://huggingface.co/IFM/Jais-3-3.7B-GGUF | Card lists BF16 10.13 GB, Q8_0 5.39 GB, Q6_K 4.16 GB, Q5_K_M 3.64 GB; the Q4_K_M file is 3.16 GB (repository file list, 3,156,606,176 bytes). The owner ran the Q4_K_M file with llama.cpp on an RTX 3050 (see the benchmark below). |

Excluded after checking: Falcon-H1-3B-Instruct, whose Hugging Face license field is `falcon-llm-license` (custom TII terms, fails rule 3). Community Arabic fine-tunes of Qwen3 (for example on featherless.ai) are not used until their training data and license are checked.

## Benchmark on the RTX 3050

Reported by the owner, measured on an RTX 3050 with 6 GB of GPU memory, llama.cpp and Q4_K_M files. These are the owner's figures as given; I did not run them, and the 14-tool test set and the prompts used are not in this repository. "Memory" is the measured GPU memory (the same figures are in `core/src/tiers.rs`), not the file size: the Q4_K_M files in the Hugging Face repositories are 2.50 GB (Qwen3-4B), 5.03 GB (Qwen3-8B) and 3.16 GB (Jais-3-3.7B).

| Model | Memory | Average time | Tools correct | Notes |
|-------|--------|--------------|---------------|-------|
| Qwen3-4B | 3.7 GB | 1.3 s | 13 of 14 | |
| Qwen3-8B | 5.8 GB | 2.6 s | 14 of 14 | `core/eval/prompts-dialects.jsonl` (30 Egyptian and Gulf prompts): 26 of 26 tool choices correct, 0 leaked characters. Reported for the same GPU; the result files are not in this repository. Reply quality not judged by a native speaker. |
| Jais-3-3.7B | 4.0 GB | 10.7 s | not reported | Slowest answer hit the 120 s timeout. Best Arabic in the owner's judgement (not a `core/eval/` score). |

CPU only, with other work running on the machine (so the times are noisy and not a clean comparison):

| Model | Time | Notes |
|-------|------|-------|
| Qwen3-1.7B Q8_0 | 4.0 s | |
| Qwen3-4B | 7.3 s | Chosen for the Lite tier. The quantization was not stated in the report. |

Qwen3-1.7B Q8_0 file: the official repository `Qwen/Qwen3-1.7B-GGUF` (already listed above) contains exactly one model file, `Qwen3-1.7B-Q8_0.gguf` (1,834,426,016 bytes), plus `LICENSE`, `README.md` and `params`. The `LICENSE` is the Apache License 2.0, the card header says `license: apache-2.0`, and the repository is not gated (read 2026-10-31). Which repository the benchmarked file came from was not stated; if it was not this one, that repository needs its own license check.

Owner's decision: on GPUs big enough for both, Jais writes and explains and Qwen handles commands. On a 6 GB GPU, Qwen3-8B alone. This matches the tiers in [installer.md](installer.md). It does not change rule 3 or the `core/eval/` requirement; the Lite model also stays open until the CPU benchmark is complete.

## Speech to text

| Model | Size | License | Status | Source | Training data / notes |
|-------|------|---------|--------|--------|-----------------------|
| Whisper (OpenAI) | tiny 39M, base 74M, small 244M, medium 769M, turbo 809M | MIT (code and weights, per README) | verified 2026-10-30 (GitHub `LICENSE`: MIT, copyright 2022 OpenAI) | https://github.com/openai/whisper | VRAM: small ~2 GB, medium ~5 GB, turbo ~6 GB (from the earlier README read, not re-read). Training data is not disclosed by OpenAI. The per-size Hugging Face cards were not read this round (the metadata labels differ: large-v3 `apache-2.0`, large-v3-turbo `mit`; both fit rule 3). |
| Cohere Transcribe Arabic (`cohere-transcribe-arabic-07-2026`) | 2B (page) | Apache-2.0 per HF metadata; card text not read | **unverified**: the repo is gated (`auto`), so the card and files return HTTP 401 without login, and the file list has no LICENSE file | https://huggingface.co/CohereLabs/cohere-transcribe-arabic-07-2026 | The public page asks the user to log in and "agree to share your Hugging Face information to access this model"; the full gate terms were not read. Arabic and English, fine-tuned from the also-gated `cohere-transcribe-03-2026`. Training data: not read (card not accessible). Gating is an access condition that rule 3 and offline install need legal review; worth benchmarking only if that is acceptable. |

## Text to speech

| Model | Size | License | Status | Source | Training data / notes |
|-------|------|---------|--------|--------|-----------------------|
| SILMA TTS v1 | 150M | Code MIT, weights Apache-2.0 | verified 2026-10-30 (HF card "License" section: "Model Weights: Apache-2.0" plus header `license: apache-2.0`; GitHub `LICENSE`: MIT. The HF repo has no LICENSE file) | https://huggingface.co/silma-ai/silma-tts, https://github.com/SILMA-AI/silma-tts | Card: "pretrained from scratch using tens of thousands of hours of high-quality public and proprietary data"; datasets and their licenses are not named. Built on F5-TTS architecture (v1.1.7 code). The card's disclaimer says "By using this voice cloning ability, you agree to" consent, no-fraud, no-misinformation and disclosure rules; that is a use condition outside the license text and needs legal review under rule 3. It uses CATT (see "Vocoder and tashkeel" below) and NeMo-text-processing, whose license was not checked. Arabic (MSA) and English. |

Habibi-TTS weights, one row per checkpoint. The Hugging Face repo header says `license: cc-by-nc-sa-4.0` for the whole repo, with no LICENSE file. The card note and the GitHub README say the unified, SAU and UAE models are CC-BY-NC-SA-4.0 ("restricted by SADA and Mixat") and ALG, EGY, IRQ, MAR and MSA are Apache 2.0. The paper (https://arxiv.org/abs/2601.13802, §2.4) says every checkpoint was initialised from F5-TTS, and the F5-TTS card (https://huggingface.co/SWivid/F5-TTS) is `cc-by-nc-4.0` with Emilia training data. Whether the Apache claim holds for models derived from non-commercial weights is not settled by any source read, so the five "Apache" checkpoints are left unverified and must not be used until that is resolved with the authors or legal review.

| Checkpoint | License as stated | Status | Source |
|------------|-------------------|--------|--------|
| Specialized/ALG | Apache-2.0 (card note) vs repo header CC-BY-NC-SA-4.0 | **unverified**: conflicting statements, NC base model | https://huggingface.co/SWivid/Habibi-TTS |
| Specialized/EGY | same | **unverified** (same reason) | same |
| Specialized/IRQ | same | **unverified** (same reason) | same |
| Specialized/MAR | same | **unverified** (same reason) | same |
| Specialized/MSA | same | **unverified** (same reason) | same |
| Specialized/SAU | CC-BY-NC-SA-4.0 | verified 2026-10-30 (card note); excluded, fails rule 3 | same |
| Specialized/UAE | CC-BY-NC-SA-4.0 | verified 2026-10-30 (card note); excluded, fails rule 3 | same |
| Unified | CC-BY-NC-SA-4.0 | verified 2026-10-30 (card note); excluded, fails rule 3 | same |
| Habibi-TTS code | MIT | verified 2026-10-30 (GitHub `LICENSE`, copyright 2026 Yushen Chen) | https://github.com/SWivid/Habibi-TTS |

Habibi training data (paper §2.2, all checkpoints together; the sources I read do not map datasets to individual checkpoints): MASC, MGB-2, MGB-3, MGB-5, SADA, Mixat, FLEURS, Omnilingual ASR Corpus, UAE-100K, UAE-Nexdata, Darija-S2T, DarijaTTS-clean, Jordan-Audio, plus in-house data and manually transcribed public recordings, about 1,857 hours. Initialised from F5-TTS, pretrained on about 95K hours of Chinese and English. The licenses of these datasets were not checked.

Excluded: Arabic-TTS-Spark (no commercial use), nipponjo/tts_arabic (unclear license, XTTS-derived voices), phoonnx Miro (CC BY-NC-ND), XTTS (non-commercial). These exclusions come from earlier notes and were not re-read this round.

## Vocoder and tashkeel

Vocos turns a mel spectrogram into audio. CATT adds Arabic diacritics (tashkeel) to text before it is spoken.

| Model | Size | License | Status | Source | Training data / notes |
|-------|------|---------|--------|--------|-----------------------|
| Vocos `charactr/vocos-mel-24khz` | 13.5M parameters (GitHub README); `pytorch_model.bin` is 54,365,991 bytes | MIT | verified 2026-10-31 (card header `license: mit`; the HF repo has no LICENSE file; GitHub `LICENSE`: MIT, copyright 2023 Charactr Inc.) | https://huggingface.co/charactr/vocos-mel-24khz, https://github.com/gemelo-ai/vocos | The card's License section says "The code in this repository is released under the MIT license"; the repository holds the weights, and the header covers it. Training data: LibriTTS, 1M iterations (GitHub README table); LibriTTS is English speech and its page at https://www.openslr.org/60/ says CC BY 4.0. The HF card itself does not name the data. Arabic quality of an English-trained vocoder is unmeasured. 24 kHz, 100 mel bands (`config.yaml`). |
| CATT code (`abjadai/catt`) | n/a | Apache-2.0 | verified 2026-10-31 (GitHub `LICENSE`, copyright 2025 Abjad AI) | https://github.com/abjadai/catt | The `LICENSE` was CC BY-NC-SA 4.0 from 2024-07-03 and was replaced by Apache-2.0 on 2025-07-18; the README has a "License Change: CC-BY-NC to Apache 2.0" section. The PyPI package `catt-tashkeel` 1.0.2 carries the Apache classifier. |
| CATT encoder-decoder weights (`best_ed_mlm_ns_epoch_178.pt`, 88,403,510 bytes; ONNX `ed_model_onnx.zip`) | about 88 MB | Not stated for the weights | **unverified**: the weights are assets of GitHub release v2, published 2024-07-04 while the repository was CC BY-NC-SA 4.0; the release notes give no license, and the 2025 change is described for "this repository" | https://github.com/abjadai/catt/releases/tag/v2 | Better diacritics accuracy; the README recommends it over the encoder-only model. |
| CATT encoder-only weights (`best_eo_mlm_ns_epoch_193.pt`, 75,762,232 bytes; ONNX `eo_model_onnx.zip`) | about 76 MB | same | **unverified** (same reason) | same | Faster; the README recommends it for speed. |

CATT training data (paper, https://arxiv.org/abs/2407.03236, §3): fine-tuned on the Tashkeela corpus (1,658,325 samples before filtering, 1,330,539 after), which the paper says is about 98.85% text from 97 books of the Shamela library, "an Islamic electronic library", mixing classical and Modern Standard Arabic. The Tashkeela project page on SourceForge lists GPL-2.0 for the corpus. The character-level BERT was pretrained on 18,543,025 samples "scraped from various sources, including X and online news websites"; the paper gives no licenses or site list. Noisy-Student training added 1M pseudo-labelled sentences from that scraped set. The weights therefore come from mostly classical and religious-library text plus scraped posts and news. CATT only adds diacritics and ScarfeeOS adds no religious content on top, but this training data needs legal review before the weights ship. The release's `dataset.zip` is the Tashkeela split. The CATT benchmark set (742 sentences scraped from a news site in 2023, per the paper) appears to be in the repository's `benchmarking` folder; its terms were not read.

## Training data

| Dataset | License | Status | Source | Notes |
|---------|---------|--------|--------|-------|
| Mozilla Common Voice Scripted Speech 27.0, Arabic | CC0-1.0 | verified 2026-10-30 (license field and "Licensing" section on the dataset page) | https://mozilladatacollective.com/datasets/cmu5wom3z00ceo107guxtg4xa | Read speech, released 2026-09-17: 136,832 clips, 158.27 hours (93.34 validated), 1,679 speakers, from 1,801,562 sentences. Forbidden usage on the page: determining speaker identity, and re-hosting or re-sharing the dataset, so it cannot ship with ScarfeeOS. "Restrictions/Special Constraints: None provided." Intended use: ASR training and evaluation, language learning. |
| Own recordings | Owned by the project | n/a | n/a | Planned for the assistant voice (about 2 to 5 hours, Arabic and English). |

## Inference runtimes (source code, not models)

| Project | License | Status | Source | Notes |
|---------|---------|--------|--------|-------|
| whisper.cpp | MIT | verified 2026-10-30 (`LICENSE` on `master`, copyright 2023-2026 The ggml authors) | https://github.com/ggml-org/whisper.cpp | Vendored third-party code under `ggml/` was not individually checked. |
| llama.cpp | MIT | verified 2026-10-30 (`LICENSE` on `master`, copyright 2023-2026 The ggml authors) | https://github.com/ggml-org/llama.cpp | Has a `vendor/` directory (cpp-httplib, nlohmann json, miniaudio, stb, others) and `licenses/LICENSE-jsonhpp`. The README calls cpp-httplib and nlohmann json MIT; the other vendored licenses were not individually checked. |

## Open items
- Habibi-TTS: ask the authors whether the Apache-2.0 claim for ALG, EGY, IRQ, MAR and MSA is valid given the CC-BY-NC F5-TTS initialisation, and have the training-data licenses (MGB-2, MASC, SADA and others) reviewed.
- Cohere Transcribe Arabic: log in to Hugging Face to read the card, the gate terms and any license file, then re-check. Decide whether a gated download is acceptable.
- SILMA: ask the authors which datasets were used; legal review of the voice-cloning disclaimer; check the NeMo-text-processing license.
- CATT weights: ask Abjad AI whether the 2025-07-18 change to Apache-2.0 covers the v2 release weights and ONNX files, and have the Tashkeela (GPL-2.0, mostly Shamela books) and scraped pretraining data reviewed.
- Vocos: nothing blocks the license; test whether a vocoder trained on LibriTTS gives acceptable Arabic.
- Whisper: read the per-size Hugging Face cards (tiny to turbo) to meet the per-size rule.
- Jais-3-3.7B: read the tech report (blocked by a Cloudflare challenge on 2026-10-31; open it in a browser or ask IFM) for a training-data statement. GGUF loading in llama.cpp is done (owner's run).
- Run Qwen3-4B, Qwen3-8B and Jais-3-3.7B through `core/eval/` for Arabic quality; Phi-4-mini-instruct has no 6 GB measurement at all.
- Lite tier: finish the CPU benchmark (the 7.3 s Qwen3-4B figure was taken with other work running) and confirm the quantization.
- Qwen3-1.7B Q8_0: confirm the benchmarked file came from `Qwen/Qwen3-1.7B-GGUF`.
- Record the training-data statement again for the chosen models: Qwen3-8B and Qwen3-4B (cards disclose none), Jais-3-3.7B (not disclosed on the card; report unread).
- Vendored licenses inside llama.cpp and whisper.cpp for the SBOM.

## Counts (2026-10-31)
34 entries: 26 verified, 8 unverified. The 8 unverified are Cohere Transcribe Arabic, the Habibi ALG, EGY, IRQ, MAR and MSA checkpoints, and the two CATT weight files. Of the 26 verified, 3 (Habibi SAU, UAE, Unified) are verified as non-commercial and excluded by rule 3. Entries counted: 6 Qwen3 sizes, 6 Qwen3 GGUF repositories, Phi-4-mini-instruct, Jais-3-3.7B, Jais-3-3.7B-GGUF, Whisper, Cohere Transcribe Arabic, SILMA TTS, 8 Habibi checkpoints, Habibi code, Vocos mel-24khz, CATT code, CATT encoder-decoder weights, CATT encoder-only weights, Common Voice Arabic, whisper.cpp, llama.cpp. Own recordings and the excluded models are not counted. The benchmark tables add no entries.
