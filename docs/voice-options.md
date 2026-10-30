# Arabic voice options

Candidates to replace or sit beside SILMA TTS for the Arabic assistant voice, with a plan for a project-owned voice. Policy: weights, code and required dependencies all Apache-2.0 or MIT, no custom or use-restricted terms (see [ip-and-licensing.md](ip-and-licensing.md) rules 2 and 3). Nothing here recommends a default, and no option below is approved for shipping. This page does not change `docs/models.md`.

Status wording follows `docs/models.md`: **verified** = the license statement was read on the primary source (model card, repository file or package metadata) on the date shown. **unverified** = not read, gated, or the primary sources conflict. "Verified" says what the license says, not that the option passes rule 3.

All checks were made on 2026-10-31 by reading Hugging Face cards and file lists, GitHub files and PyPI metadata. No model was downloaded and no audio was heard or generated, so nothing here says how any voice sounds, how well it pronounces a dialect, or whether it fits in 6 GB (see "Not tested").

## Summary

- **No option passes today.** Every Arabic voice found with an Apache-2.0 or MIT header fails at least one of: no training-data statement, unstated consent of the voice owner, a base model with use restrictions or non-commercial terms, or a runtime that imports GPL code.
- **Closest on paper:** the Kokoro/StyleTTS2 family fine-tuned for Arabic. Egyptian: KemeTone. Gulf (Saudi, Najdi): Nabra-Saudi-82M and Nabra-Saudi-7M. Levantine: Sofelia. MSA: Nabra-82M. They are small (82M, or 7.5M for Nabra-Saudi-7M), say they run on CPU, and their card headers say Apache-2.0. None names its Arabic training corpus, and the Kokoro/StyleTTS2 runtime pulls in GPL code (see "The Kokoro runtime problem").
- **No safe Emirati, Kuwaiti, Qatari or Bahraini voice was found.** The Emirati voices found have no data statement or sit on a base with restrictions.
- **Not yet checkable:** MOSS-TTS-Nano, Chatterbox Multilingual and VoxCPM2 list Arabic and have permissive headers, but have no dialect statement or no Arabic training-data statement, and the Arabic quality is unmeasured.
- **Realistic route:** a project-owned voice from the owner's recordings (see "Project-owned voice"), fine-tuned from a base whose provenance has been cleared, with the runtime rewritten so no GPL code is imported. In parallel, ask the authors of the Kokoro-family models the questions in "Open items".

## What counts as passing

For each option the following were checked, in this order. A failure at any step stops the option.

1. Weights license is Apache-2.0 or MIT, read on the primary source, with no extra terms in the card (a research-only or "inform listeners" clause counts as a custom term).
2. The base model it was fine-tuned from also passes. A fine-tune cannot be more permissive than its base: a header saying Apache-2.0 on a fine-tune of a non-commercial base is not accepted (same reasoning as the Habibi rows in `docs/models.md`).
3. Code and every dependency needed at run time pass rule 2. GPL code that is imported into the same process fails, as `Unidecode` does for SILMA in `core/tts/README.md`.
4. A training-data statement exists that can be recorded in `docs/models.md`, including who the voice belongs to and whether they consented.
5. Dialect coverage, size, and 6 GB GPU or CPU fit are recorded, from the card where not measured.

## Options considered

### Egyptian

| Option | Weights license | Dialect | Size | 6 GB GPU / CPU | Blockers | Status |
|--------|-----------------|---------|------|----------------|----------|--------|
| KemeTone (`Rabe3/kemetone`), Kokoro fine-tune of Nabra-82M | Apache-2.0 (card header and "Licence" section; the repo has no LICENSE file) | Cairene, one female voice | 81.8M parameters, 327 MB fp32 file, 331 MB repo | Card: CPU or any CUDA GPU, about 300 MB of VRAM. Not measured | No training-data statement. The card says the voice "is modelled on a real person" and gives no consent or source. Needs diacritized input (a diacritizer is a dependency; CATT's license is still unchecked in `docs/models.md`). Base chain has the Nabra and Kokoro/StyleTTS2 issues below. Runtime imports GPL code. | verified license 2026-10-31; **fails** step 4 |
| Nabra-82M-Egyptian-da7ee7 (`masterofaudio2077/...`) | Apache-2.0 header; the card itself says "the data card of the dataset does not state a license" | Egyptian, one male voice | 81.8M parameters | Card: runs on one GPU. Not measured | The training dataset has no license, and the card describes it as a separated, 8 kHz-sourced voice, so the owner and their consent are unknown. Card says espeak-ng has no Egyptian model, so some letters come out in MSA form, and gives WER 0.550 / CER 0.260 against 0.432 / 0.234 for the real recordings. | verified 2026-10-31; **fails** step 4 |
| NAMAA-Egyptian-TTS (`NAMAA-Space/NAMAA-Egyptian-TTS`) and `oddadmix/chatterbox-egyptian-v0` | MIT headers; base `ResembleAI/chatterbox` (MIT) | Egyptian, via Chatterbox Multilingual with a reference voice | Chatterbox repo is 13.9 GB in total | Not stated in the cards, not measured | The NAMAA card says the dialect comes from "specialized configuration, prompting, and curated usage patterns", not from training, so it adds nothing over the base. Base blockers in the Chatterbox row below. | verified 2026-10-31; **fails** step 4 |
| MasriSwitch-TTS (`Tarek737/MasriSwitch-TTS`), fine-tune of SILMA | Apache-2.0 header | Egyptian-English code-switching | Same as SILMA (150M) | Not stated | Built on SILMA, so it keeps SILMA's open questions (data not named, voice-cloning terms, dependencies). Its own card says the improvement target was not met (code-switch WER 61.38% to 61.34%) and calls itself experimental. The data it adds is CC BY 4.0 synthetic speech. | verified 2026-10-31; not a way out of the SILMA blockers |
| lahgtna-omnivoice-egyptian-v3 (`ehabnegm/...`) | Apache-2.0 header | Egyptian | 2.5 GB | Not stated | Base chain ends in OmniVoice, whose card says "The pre-trained model is licensed under the CC-BY-NC due to constraints from its training data (e.g., Emilia)". | verified 2026-10-31; **excluded** by step 2 |
| Habibi-TTS EGY | Apache-2.0 per card note, CC-BY-NC-SA-4.0 per repo header | Egyptian | See `docs/models.md` | Not stated | Already in `docs/models.md` as unverified: initialised from non-commercial F5-TTS. | unverified (unchanged) |

### Gulf

| Option | Weights license | Dialect | Size | 6 GB GPU / CPU | Blockers | Status |
|--------|-----------------|---------|------|----------------|----------|--------|
| Nabra-Saudi-82M (`oddadmix/Nabra-Saudi-82M`) | Apache-2.0 (card header and "License & attribution"; no LICENSE file) | Saudi, one female voice | 81.8M parameters; 327.5 MB file | Card: CPU samples; not measured | Card names 146.5 h of "single-speaker Saudi speech" (34,404 clips) but not its source, owner or license. A second checkpoint adds 13.4 h of synthetic Saudi phone conversations, with no generator named. Vendored patched Kokoro package; the runtime imports GPL code. | verified 2026-10-31; **fails** step 4 |
| Nabra-Saudi-7M (`oddadmix/Nabra-Saudi-7M`) | Apache-2.0 header | Saudi | 7.48M parameters, 30 MB (card text of the 82M teacher) | Built for on-device; not measured | Distilled from Nabra-Saudi-82M, so it inherits the same data question. | verified header 2026-10-31; **fails** step 4 |
| Fasee7-Najdi-Small (`wittify/Fasee7-Najdi-Small`), LoRA on VoxCPM2 | Apache-2.0 (card section "License", consistent with VoxCPM2) | Najdi (Saudi) only | 72 MB LoRA on a 2.29B-parameter base (4.96 GB repo) | VoxCPM2 card lists about 8 GB of VRAM, so it does not fit 6 GB as published. Not measured | Training data is "High-quality in-house Najdi Arabic dataset", with no owner or consent statement. The card warns of catastrophic forgetting outside Najdi. | verified 2026-10-31; **fails** step 4 and the 6 GB estimate |
| A3S (`MahmoudIbrahim/A3S`), Qwen3-TTS 0.6B Base with Arabic added | Apache-2.0 header; base `Qwen/Qwen3-TTS-12Hz-0.6B-Base` (Apache-2.0) | Saudi, Emirati and MSA, three named speakers | 2.5 GB repo | Not stated, not measured | No training-data statement and no statement on who the three speaker identities are. | verified header 2026-10-31; **fails** step 4 |
| `vadimbelsky/emirati-vits-male-1.0`, VITS | Apache-2.0 | Emirati, one male voice | "about 500 MB for model and dependencies" (card) | Card: CPU is slow, GPU recommended | "Fine-tuned on 70 hours of bilingual audio" with no source; the base is not named. G2P comes from a custom NeMo fork whose license was not read. | verified 2026-10-31; **fails** step 4 |
| `vadimbelsky/arabic-emirati-female-piper`, Piper | MIT | Emirati, one female voice | ONNX, size not read | 63 MB ONNX file; not measured | Card says only "trained using the Piper TTS training pipeline": no data, no base, no consent. Piper's phonemizer is espeak-ng (GPL-3.0). | verified 2026-10-31; **fails** step 4 |
| `vadimbelsky/qwen3.5-TTS-Emirati` | Apache-2.0 header; base `Qwen3-TTS-12Hz-1.7B-Base` | Emirati | 4.5 GB | Not stated | Qwen3-TTS lists 10 languages and Arabic is not one; the card says Arabic was added by changing the codec vocabulary. No data statement. | verified header 2026-10-31; **fails** step 4 |
| NAMAA-Saudi-TTS (`NAMAA-Space/NAMAA-Saudi-TTS`) | MIT; base Chatterbox | Saudi | 5.4 GB | Not stated | As for the NAMAA Egyptian row. | verified 2026-10-31; **fails** step 4 |
| `TheBOrganization/Arabic_TTS`, VibeVoice-based | Apache-2.0 header | Arabic | 5.4 GB | Not stated | VibeVoice's card says the model "is limited to research purpose use" and adds out-of-scope uses beyond the MIT text, so the base has use-restricted terms. | verified 2026-10-31; **excluded** by step 2 |
| `AhmedEladl/saudi-tts` | Apache-2.0 header | Saudi | 5.6 GB | Not stated | Installs the Coqui TTS package. No base model or data is named. If it is XTTS-derived, the XTTS license (`coqui-public-model-license`, non-commercial) applies; this could not be confirmed. | unverified base; **excluded** until the base is shown |
| VoxCPM2 Arabic-Dialects (`VoxCPM-Kernel/VoxCPM2-Arabic-Dialects`) | Apache-2.0 header | "Arabic dialects" per the name | 6.6 GB | VoxCPM2 base about 8 GB | The repo is gated (`manual`), so the card was not read. | unverified |
| Habibi-TTS SAU and UAE | CC-BY-NC-SA-4.0 | Saudi, Emirati | Already in `docs/models.md` | n/a | Excluded there by rule 3. | verified, excluded |

### Levantine, MSA and multilingual bases

| Option | Weights license | Dialect | Size | 6 GB GPU / CPU | Blockers | Status |
|--------|-----------------|---------|------|----------------|----------|--------|
| Kokoro-82M (`hexgrad/Kokoro-82M`) | Apache-2.0 (card header; no LICENSE file) | No Arabic | 82M, 327 MB | Not stated in the parts read; not measured | The base of the Arabic family above. Card training data: "permissive/non-copyrighted audio" (Apache or MIT audio, plus synthetic audio "generated by closed TTS models from large providers", none from open TTS models or voice clones), a few hundred hours, about 1000 A100-hours. It also lists CC BY audio (Koniwa under 1 h, SIWIS under 11 h), so attribution is owed. The provider terms of the closed TTS services were not checked. The card's base model is `yl4579/StyleTTS2-LJSpeech`, whose terms are in "StyleTTS2 pre-trained models". | verified 2026-10-31 |
| Nabra-82M-v0.1 (`oddadmix/Nabra-82M-v0.1`) | Apache-2.0 (header and "License & Attribution") | MSA, one female voice (`af_msa`) | 82M, 328 MB | Card: real-time factor under 1 on CPU. Not measured | No training-data statement. Needs diacritized input, using camel-tools (MIT per PyPI; its data license was not read) or CATT. It is the base of KemeTone, Nabra-Saudi and the Egyptian fine-tune above. | verified 2026-10-31; **fails** step 4 |
| Sofelia-TTS-82M (`hamdallah/Sofelia-TTS-82M`) | Apache-2.0 header (no LICENSE file) | Palestinian (Levantine), one female voice | 82M, 328 MB | Card: real-time factor 0.15 to 0.34 on 4 CPU threads | Card names only "Sofelia Palestinian TTS" as the voice data, with no link, license or consent. | verified 2026-10-31; **fails** step 4 |
| MOSS-TTS-Nano (`OpenMOSS-Team/MOSS-TTS-Nano`) | Apache-2.0 header on the model, tokenizer and ONNX repos; GitHub root `LICENSE` is Apache-2.0. The HF README License section reads "will follow the license specified in the root LICENSE file", which is ambiguous on its own | Arabic is listed among 20 languages; no dialect statement | 0.1B parameters; ONNX CPU build 673 MB + 91 MB | Card: streaming on a 4-core CPU | No training-data statement for the TTS weights (the tokenizer card says 3 million hours, without sources). Dialect, Egyptian and Gulf quality unknown. Python dependency licenses were not read. | license verified 2026-10-31; **unverified** for rule 3 (data) |
| Chatterbox Multilingual (`ResembleAI/chatterbox`) | MIT (header; PyPI `chatterbox-tts` and `resemble-perth` are MIT) | Arabic is one of 23 languages; no dialect statement | 13.9 GB repo | Not stated | Card: "Trained on 0.5M hours of cleaned data" and "Prompts are sourced from freely available data on the internet": no sources, no per-source licenses. Every output carries the Perth watermark. | license verified 2026-10-31; **unverified** for rule 3 (data) |
| VoxCPM2 (`openbmb/VoxCPM2`) | Apache-2.0 | Arabic among 30 languages, no dialect | 2.29B parameters, 4.96 GB | Card: about 8 GB VRAM, so not 6 GB | "2M+ hours multilingual speech" with no sources. | license verified 2026-10-31; **fails** the 6 GB estimate |
| Qwen3-TTS (`Qwen/Qwen3-TTS-12Hz-0.6B-Base`) | Apache-2.0 | Arabic is not in the supported list (10 languages) | 0.91B parameters, 2.5 GB | Not measured | No Arabic in the base. Every Arabic version is a third-party fine-tune (A3S, the Emirati one). | verified 2026-10-31 |
| Piper voice `ar_JO` kareem, low and medium (`rhasspy/piper-voices`) | MIT on the repo; each model card says dataset "License: See URL" | Jordanian, one voice | 63 MB (low) | Small; not measured | Dataset URL is `github.com/AliMokhammad/arabicttstrain`, which has no license file and no license in the GitHub API. Fine-tuned from the English lessac voice. Phonemizer is espeak-ng. | verified 2026-10-31; **fails** step 4 |

### Excluded

| Option | Reason | Evidence |
|--------|--------|----------|
| MMS-TTS Arabic (`facebook/mms-tts-ara`) | CC-BY-NC-4.0 | HF header |
| ArTST family, CLArTTS SpeechT5 (`MBZUAI/ArTST*`, `MBZUAI/speecht5_tts_clartts_ar`) | CC-BY-NC-4.0 | HF headers |
| XTTS-v2 (`coqui/XTTS-v2`) | `coqui-public-model-license`, custom terms; named in rule 3 | HF header |
| Spark-TTS and Arabic derivatives, OmniVoice and derivatives, F5-TTS and derivatives | Non-commercial base (OmniVoice card: CC-BY-NC; `SWivid/F5-TTS` header: cc-by-nc-4.0, Emilia data) | HF cards; Spark-TTS was excluded in `docs/models.md` from earlier notes and not re-read |
| VibeVoice-1.5B (`microsoft/VibeVoice-1.5B`) | MIT header but "limited to research purpose use" and extra out-of-scope uses | HF card |
| `fadi77/StyleTTS2-LibriTTS-arabic` | MIT header, but it starts from the StyleTTS2 LibriTTS model (use condition, below) and its 22 h audiobook dataset (`fadi77/arabic-audiobook-dataset-24khz`) could not be read | HF card; dataset page not read |
| `facebook/tts_transformer-ar-cv7` | No license in the card metadata (trained on Common Voice 7, fairseq) | HF metadata |
| Piper itself as software | See "Piper" | below |

## The Kokoro runtime problem

The model files are separate from the software that runs them, and the software is where the family fails.

- `kokoro` 0.9.4 (Apache-2.0 per PyPI) requires `misaki[en]`. `misaki` 0.9.4 (Apache-2.0) lists, for the `en` extra, `phonemizer-fork` (GPL-3.0-or-later per PyPI classifier), `espeakng-loader` (the repository `LICENSE` is MIT; PyPI metadata is blank, and the espeak-ng it loads is GPL-3.0 per the GitHub API for `espeak-ng/espeak-ng`), and `num2words` (LGPL per PyPI).
- KemeTone, Nabra and Sofelia import these in the same Python process. That is the same situation as `Unidecode` for SILMA, which `core/tts/README.md` records as a failure under rule 2.
- Possible fixes, none tried: (a) call the `espeak-ng` program as a separate process and write the small front end (normalisation, lexicon, phoneme clean-up) as original code, as the model cards describe it; (b) write an original rule-based Arabic grapheme-to-phoneme step for diacritized text, but then the model has to be trained on that step's output, because the card of KemeTone says weights trained on one phonemiser "will degrade" on another; (c) run only the model graph through ONNX Runtime (MIT per PyPI) from the Rust core.
- Fix (a) does not change the data question. Fixes (b) and (c) need a retrain or an export that nobody has done here.

### StyleTTS2 pre-trained models

Kokoro lists `yl4579/StyleTTS2-LJSpeech` as its base. The StyleTTS2 code is MIT (`LICENSE`, 2023, Aaron (Yinghao) Li), but its README says of the pre-trained models: "Before using these pre-trained models, you agree to inform the listeners that the speech samples are synthesized by the pre-trained models, unless you have the permission to use the voice you synthesize". The README's `License` section repeats it. The Hugging Face repos `yl4579/StyleTTS2-LJSpeech` and `StyleTTS2-LibriTTS` have no license field. That is a use condition outside Apache-2.0 or MIT and, read strictly, a custom term under rule 3. The README also says the condition applies when the voice is not in the training set. Whether it reaches Kokoro and its fine-tunes is a question for the Kokoro author and for legal review, not something this page settles. The same README says the helper models in `Utils/` (text aligner trained on LibriTTS, JVS and AiShell; pitch extractor on LibriTTS; PL-BERT on English Wikipedia) are pre-trained modules, so any training that reuses them carries the same question.

## Piper

- `rhasspy/piper` is MIT per the GitHub API, but it was archived on 2025-08-26 and its README says "Development has moved" to `OHF-Voice/piper1-gpl`, which the GitHub API reports as **GPL-3.0**. New Piper development is therefore GPL; the MIT repository is frozen and not maintained.
- Voices in `rhasspy/piper-voices` carry `license: mit` on the repository header, but each voice has its own model card pointing at its dataset (kareem: "License: See URL", and the URL has none). The header does not settle the voice.
- The only Arabic voice there is `ar_JO` kareem, one voice, low and medium quality (16 kHz and 22.05 kHz).
- The phonemizer in both generations is espeak-ng, the same GPL situation as above.
- Conclusion: Piper's current license does not fit if the project builds on the maintained code, and the frozen MIT code plus kareem still lacks a dataset license. Not usable as is.

## Project-owned voice

The goal is a voice the owner recorded, with a written license and consent, trained on a base that passes, and shipped under Apache-2.0. All numbers below are **estimates** from the figures quoted from primary sources; none was measured.

### Data points from primary sources

- StyleTTS2 README: fine-tuning on about 1 hour (about 1k samples) for 50 epochs "took about 4 hours to finish on four NVidia A100", with quality "slightly worse" than full training. Out-of-memory failures are reported on small GPUs during the speech-language-model adversarial stage; the stage can be skipped with lower quality. A single-GPU script with fp16 exists.
- Kokoro card: about 1000 A100-hours in total, about $1000, a few hundred hours of audio. That is about $1 per A100-hour, which this page uses only as a unit for rough costs.
- Nabra-Saudi card: 40k steps on 146.5 h for a dialect fine-tune.
- Habibi paper figures are in `docs/models.md`: about 1,857 h of data for a dialect-capable F5-based model.
- `docs/models.md` already plans 2 to 5 hours of the owner's recordings (Arabic and English) and lists Common Voice Arabic 27.0 (CC0, 158.27 h, 93.34 validated, 1,679 speakers).

### Path A: fine-tune a cleared Kokoro-family base on the owner's recordings

Preferred, because 2 to 5 hours is enough to adapt a pre-trained model and far too little to train one from scratch.

| Step | Work | Estimate |
|------|------|----------|
| 0. Clear the base | Ask the Nabra author (and Kokoro's) for a written training-data statement and the source and license of the Arabic corpus; get the StyleTTS2 pre-trained-model condition reviewed. If the answer is no, go to Path B. | 1 to 4 weeks elapsed, mostly waiting; about 2 days of effort |
| 1. Consent and license | Owner signs a grant of the recordings and the trained voice to the project (Apache-2.0 for the weights), covering commercial use, duration and withdrawal. The consent fields in `core/tts/README.md` apply. | 1 day, plus legal review |
| 2. Recording | 2 to 5 hours of clean speech, one microphone, a quiet room, 48 kHz capture, Arabic and English scripted from original sentences. Raw takes are usually 2 to 3 times the final speech, so 6 to 15 hours in the booth over several sessions. | 1 to 2 weeks part-time |
| 3. Transcripts | Draft with Whisper (MIT), then a native speaker corrects each line. 3 hours is about 2,000 to 2,500 utterances of 4 to 5 s. At 1 minute each that is about 35 to 40 hours. Decide per language whether to write diacritics: the Nabra-Saudi card says an MSA diacritizer made dialect results worse, and the KemeTone card says undiacritized input hurts the vowels, so test both. | 1 to 2 weeks |
| 4. Front end | Original grapheme-to-phoneme code that calls the `espeak-ng` program (or a rule-based step), plus a pronunciation lexicon for the owner's dialect. | 1 to 3 weeks of engineering |
| 5. Training | Align with a CTC aligner trained on the owner's audio, as the Nabra training notes describe, and check it with the reversed-label control. Fine-tune on rented GPUs, not the 6 GB laptop GPU. By the StyleTTS2 figure, 3 hours is about 50 A100-hours per full run; plan 3 to 5 runs. | 150 to 250 A100-hours, about $150 to $250 at the Kokoro card's rate; 1 to 2 weeks elapsed |
| 6. Evaluation | Round-trip transcription error with Whisper on held-out sentences; blind listening by native speakers for naturalness, dialect and pronunciation; a fixed list of hard words and numbers. | 1 week, needs at least 3 native listeners |
| 7. Runtime | Export the model graph and run it from the Rust core with ONNX Runtime, so no Python or GPL package ships. Measure CPU real-time factor and 6 GB memory. | 3 to 6 weeks of engineering |
| 8. Records | Training-data statement, consent record and license into `docs/models.md`; SBOM entries. | 2 days |

Total, as an estimate: about 2 to 4 months of calendar time with one engineer and one speaker, mostly gated by steps 0, 3 and 7. Risks: 2 to 5 hours gives a single voice with limited emotional range; the quality may sit below Habibi or SILMA; the diacritics question needs a native listener.

### Path B: a fully traceable base from CC0 data

If step 0 fails, train the base too. Common Voice Arabic (CC0) is crowd-sourced read speech from 1,679 speakers on mixed microphones, so a model trained only on it will be noisier than the Kokoro line. The dataset page forbids determining speaker identity and re-hosting; whether training speaker embeddings counts is for legal review. This path also means retraining the aligner and pitch extractor (the StyleTTS2 helpers carry the condition above) and an Arabic PL-BERT, whose text source needs its own license check. As an estimate this is several times the work of Path A: 4 to 8 months, and 300 to 1000 A100-hours (about $300 to $1000 at the Kokoro rate) for the base, before the owner-voice fine-tune. The quality outcome is the least certain of all the options on this page.

### Not assessed

A VITS or Piper-style model trained from the owner's recordings was not assessed: the original VITS code license and the training-code license of the maintained Piper (GPL-3.0) were not both read, and the maintained Piper code is GPL.

## Not tested

- No audio of any option was heard. Dialect accuracy (for example Cairene /g/ and glottal stop, Najdi forms) is as stated in the cards only.
- No model was downloaded or run, so no 6 GB GPU or CPU fit was measured. Every size and hardware claim above is from a card, a file list or a file size.
- No license file existed in the Kokoro-family Hugging Face repos; each license was taken from the card header and card text.
- Dependency licenses were read for `kokoro`, `misaki`, `phonemizer-fork`, `espeakng-loader`, `num2words`, `camel-tools`, `chatterbox-tts`, `resemble-perth`, `qwen-tts`, `voxcpm` and `onnxruntime` from PyPI metadata. The wheels were not opened, so what `espeakng-loader` bundles is from its description, not from the file list.
- Gated or unreadable: the VoxCPM2 Arabic-Dialects card, the `fadi77` audiobook dataset, and GitHub code search.
- The effort and cost figures are estimates for planning only.

## Open items

- Ask the Nabra author (`oddadmix`) which corpora trained Nabra-82M, Nabra-Saudi and the Egyptian fine-tunes, who owns them, and under what licenses.
- Ask the KemeTone author whether the speaker consented to a public model, and on what corpus it was trained. Ask the Sofelia author the same for "Sofelia Palestinian TTS".
- Ask the Kokoro author whether the StyleTTS2 pre-trained-model condition reaches Kokoro and its fine-tunes, and which closed TTS services produced the synthetic audio.
- Legal review of: the StyleTTS2 condition; the Kokoro CC BY attributions (Koniwa, SIWIS); the closed-provider terms behind Kokoro's synthetic data; Common Voice's forbidden uses for TTS training.
- Check CATT and camel-tools data licenses (diacritizers needed by the Kokoro-family fronts).
- MOSS-TTS-Nano: read its dependency licenses and test Arabic quality; ask OpenMOSS for a training-data statement.
- Chatterbox Multilingual: ask Resemble AI for the 0.5M-hour data sources if it is to be considered again.
- Decide the owner-voice path (A or B) once step 0 of Path A has an answer.

## Changes needed in other files

This page changes only itself. If these results are accepted: add the Kokoro, Nabra, KemeTone, Sofelia, MOSS-TTS-Nano and Chatterbox rows (and the exclusions) to the TTS table in `docs/models.md` with their counts; update the "No Arabic voice can ship yet" line in `docs/lifecycle.md` and `docs/i18n-and-arabic.md` to point here; add a short pointer to this page in `core/tts/README.md`.
