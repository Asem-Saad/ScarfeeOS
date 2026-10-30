# SILMA speech program

`silma_speak.py` is the program that `CommandSpeaker` in `core/src/tts.rs` runs to turn text into a WAV file with SILMA TTS (code MIT, weights Apache-2.0, see `docs/models.md`). It loads the model once, speaks every sentence and writes one WAV.

```
silma_speak.py OUT_WAV LANGUAGE REF_WAV REF_TEXT_FILE      (sentences on stdin, one per line, UTF-8)
```

`CommandSpeaker` setup: program = the Python of the virtual environment below, arguments = `["<path>/silma_speak.py", "{out}", "{lang}", "<reference.wav>", "<reference.txt>"]`. The text arrives on stdin, as `tts.rs` does when the arguments have no `{in}`.

- `LANGUAGE` is `en` or `ar` and is checked, but SILMA picks the language from the script of each sentence, so it changes nothing else.
- Each sentence is synthesized on its own with `SilmaTTS().infer(...)` and the pieces are joined with a 0.3 s pause. Standard library only for the joining.
- Exit status: 0 done, 2 bad arguments or input, 3 refused, 4 missing model files or packages, 5 the model failed. The reason goes to stderr.

## Refusals

- **The bundled `ar.ref.24k.wav` is refused**, by file name (also through a link or in another case) and by its SHA-256, so a renamed copy is refused too. Its transcript is religious text, which ScarfeeOS does not provide. The reference is always an explicit argument and there is no default. Note that `pip install silma-tts` puts that file inside `site-packages/silma_tts/`; the image build should delete it.
- The reference WAV must be longer than 0 s and at most 8.0 s. SILMA cuts longer audio at 8.05 s and then transcribes it on the fly, which downloads a speech model and so breaks the no-network rule.
- The transcript file must exist, be UTF-8 and not be empty (SILMA would otherwise transcribe the audio itself).

## No network at run time

The program sets `HF_HUB_OFFLINE`, `TRANSFORMERS_OFFLINE` and `HF_DATASETS_OFFLINE` to `1` before the model loads, and first checks that every model file is already on disk. If one is missing it exits with status 4 and lists the files; it never downloads.

Files the model needs:

| Files | Where they live |
|-------|-----------------|
| `silma-ai/silma-tts`: `model.pt`, `vocab.txt` | Hugging Face cache (`HF_HOME`, default `~/.cache/huggingface`) |
| `charactr/vocos-mel-24khz`: `config.yaml`, `pytorch_model.bin` | Hugging Face cache |
| `eo_model` tashkeel models (`encoder.onnx`, `decoder.onnx`, 72 MB) | `catt_tashkeel/onnx_models/` inside the virtual environment, downloaded from a GitHub release by `catt_tashkeel` |

About 2.5 GB in all.

## Install and pre-fetch (the only steps that use the network)

Python 3.12 on Linux x86_64 is what was tried. Use a virtual environment, never the system Python.

```
python3 -m venv ~/silma-venv
~/silma-venv/bin/pip install --no-deps -r core/tts/requirements.txt
```

`--no-deps` is required. The file lists every package, but it leaves out `encodec` (CC-BY-NC) and `Unidecode` (GPL), and `vocos` and `silma_tts` still name them as dependencies, so without `--no-deps` pip installs them again. `pip check` then reports those two as missing; that is expected (see "Packages left out" below).

`cdifflib` (a dependency of `nemo_text_processing`) has no wheel, so a C compiler and the Python headers (`python3-devel` on Fedora) are needed. `pynini` has no Windows wheel on PyPI, so this route is Linux only. The requirements pin the CPU build of PyTorch from the PyTorch index.

Then fetch the model files once:

```
~/silma-venv/bin/python - <<'EOF'
from huggingface_hub import hf_hub_download
for repo, name in [("silma-ai/silma-tts", "model.pt"), ("silma-ai/silma-tts", "vocab.txt"),
                   ("charactr/vocos-mel-24khz", "config.yaml"), ("charactr/vocos-mel-24khz", "pytorch_model.bin")]:
    print(hf_hub_download(repo, name))
from catt_tashkeel import CATTEncoderOnly
CATTEncoderOnly()
EOF
```

The downloads are not checked against any hash by the libraries. Record `sha256sum` of each file when you fetch them.

## Tests

```
python3 -m unittest discover -s core/tts
```

They use a fake `silma_tts` module and need only the standard library. They cover the arguments, reading stdin, the refusals, the offline switch, the missing-files error, the WAV joining and the `encodec` stand-in. They do not run the model.

## What was really run (2026-10-30)

On a Linux x86_64 machine with no GPU, I built a virtual environment (CPU PyTorch 2.6.0 and `silma-tts` 1.0.5), fetched the files above, then ran `silma_speak.py` inside `unshare -rn` (a network namespace with no network). It spoke one English and one Arabic sentence and wrote a valid 24 kHz, 16-bit mono WAV, exit status 0. The reference was a synthetic 4 s tone, not a voice, so the audio quality was not judged and **no real voice has been heard**. With the files missing it exited 4 as designed.

Timing on 12 CPU threads: 77 s for the two sentences and 50 s for one short sentence, so most of it is start-up (the log shows the NeMo grammars being built on every start). `CommandSpeaker::DEFAULT_TIMEOUT` is 120 s, so a longer reply will time out; set a longer `with_timeout`, or keep the program running between replies, which this program does not do. `pydub` warns that `ffmpeg` is missing; WAV input worked without it.

## Packages left out: `encodec` and `Unidecode`

Checked on 2026-10-31 in a Python 3.12 virtual environment with the pinned set (CPU PyTorch 2.6.0, `silma-tts` 1.0.5), running `silma_speak.py` for real inside `unshare -rn` with one English and one Arabic sentence.

| Package | License (from the package's own metadata and license file) | Is it needed to speak? |
|---------|-----------------------------------------------------------|------------------------|
| `encodec` 0.1.1 | CC-BY-NC 4.0 (`LICENSE`, `PKG-INFO`) | No. `vocos/feature_extractors.py` does `from encodec import EncodecModel` at the top, so `import vocos` fails without it, but only `EncodecFeatures` calls it. The vocoder SILMA loads, `charactr/vocos-mel-24khz`, uses `MelSpectrogramFeatures` (its `config.yaml`). Removed with nothing in its place the program exited 4; with the stand-in below it exited 0. |
| `Unidecode` 1.4.0 | GPL-2.0-or-later (`LICENSE`, classifier) | No. `silma_tts` lists it in its metadata (`Requires-Dist: unidecode`), but no `.py` file in the installed `site-packages` imports it. With it uninstalled (and the `encodec` stand-in below in place) the program exited 0, with no replacement for it. `anyascii` (ISC) is therefore not needed and was not added. |

**The stand-in.** `stub_encodec()` in `silma_speak.py` puts a small `encodec` module into `sys.modules` before `silma_tts` is imported, but only when no real `encodec` is installed. Its `EncodecModel` raises `ImportError` if anything calls it. It is written for ScarfeeOS (Apache-2.0) and copies nothing from `encodec`. SILMA (MIT) and `vocos` (MIT) are used as installed: nothing of theirs was modified or vendored. What this does not prove: any other SILMA or vocos setting that selects the Encodec vocoder (`charactr/vocos-encodec-24khz`) would now fail, and the program never loads it.

**What was run:** (1) the full set: exit 0; (2) `encodec` and `Unidecode` uninstalled, no stand-in: exit 4 (`No module named 'encodec'`); (3) the same with the stand-in: exit 0, twice; (4) a new virtual environment built from this `requirements.txt` with `--no-deps` (so also without `setuptools`, which `pip freeze` never listed): exit 0, a valid 24 kHz 16-bit mono WAV of 5.96 s. The reference was the synthetic tone again, so audio quality was not judged. SILMA's output is not repeatable: the same setup gave 6.62 s and 6.86 s on two runs, so lengths between runs were not compared. Peak memory was 4.2 GB and a run took about 2 minutes with 4 threads, so run one at a time on a shared machine.

## `frozendict`, `soxr` and `catt_tashkeel`

Same method, same day.

| Package | License | Needed? | What happened |
|---------|---------|---------|---------------|
| `frozendict` 2.5.0 | LGPL-3.0-only (`LICENSE.txt`). The wheel is pure Python (`py3-none-any`). | At import only. `einx/_src/util/lru_cache.py` imports it; `einx` comes from `x_transformers`. | Uninstalled: `silma_tts` would not import (exit 4). With a three-line `frozendict(dict)` stand-in that logged each use: exit 0 and the log stayed empty, so it was never called for these two sentences. |
| `soxr` 1.1.0 | LGPL-2.1-or-later (`COPYING.LGPL`). The wheel's `soxr_ext.abi3.so` has libsoxr compiled in: `ldd` shows no libsoxr, only libstdc++, libm, libgcc and libc. | At import only. `librosa` and `transformers` import it. SILMA resamples with `torchaudio` and uses `librosa` only for mel filters. | Uninstalled: exit 4. With a stand-in whose `resample` raises and logs: exit 0, never called. |
| `catt_tashkeel` 1.0.2 | Apache-2.0 per the wheel classifier and README; the wheel has no license file | Yes. `silma_tts` imports it at the top, and it adds the vowel marks to every Arabic sentence (English never uses it). | Uninstalled: exit 4. Not replaced. |

Neither `frozendict` nor `soxr` was dropped from `requirements.txt`, because no stand-in is shipped for them. Whether a pure-Python LGPL package that is installed separately, and an LGPL library compiled into a Python extension, satisfy "LGPL only dynamically linked" (rule 2) is a question for the project's lawyer. The facts above are what they need. The stand-in runs only cover English and Arabic text with one reference, so another code path might still call them.

## Licenses of the dependencies

Checked against `docs/ip-and-licensing.md` rule 2, using the metadata and license files in the installed packages (134 packages in `requirements.txt`). Rule 2 says GPL/AGPL code must be a separate program that ScarfeeOS code never links to, LGPL only dynamically linked, and rule 3 allows only Apache-2.0 or MIT for models.

**Fail:** none left in `requirements.txt`. `encodec` (CC-BY-NC 4.0) and `Unidecode` (GPL-2.0-or-later) failed and were removed, see above.

**Needs a decision:**

| Package | License | Note |
|---------|---------|------|
| `catt_tashkeel` 1.0.2 | Apache-2.0 (GitHub repo and wheel classifier; the wheel has no license file) | The repo changed from CC-BY-NC to Apache-2.0. Its models are downloaded from a GitHub release, and the training data (a released dataset) and its licenses were not checked. Not in `docs/models.md`. |
| `nemo_text_processing` 1.1.0 | Apache-2.0 | Passes. Its grammar data and the Arabic rules were not checked. |
| `pynini` 2.1.6 | Apache-2.0 | Passes (OpenFst is Apache-2.0). Linux wheels only. |
| `frozendict` 2.5.0 | LGPL-3.0 | Pure Python, imported by `einx`; see the section above. Confirm against rule 2. |
| `soxr` 1.1.0 | LGPL-2.1-or-later | libsoxr is compiled into the wheel's extension, not a separate shared library; see the section above. Confirm against rule 2. |
| `charactr/vocos-mel-24khz` (model) | MIT (Hugging Face metadata) | Passes rule 3, but is missing from `docs/models.md`. |

**Pass (preferred licenses, or permissive and not on the list):** all other packages are MIT, BSD, Apache-2.0, ISC or MPL-2.0 (`certifi`, `tqdm`, `orjson` in part). Permissive but not named in rule 2: `matplotlib` and `typing_extensions` (Python Software Foundation terms), `regex` (Apache-2.0 and CNRI), `Pillow` (MIT-CMU), `wget` (public domain), `python-dateutil` (BSD and Apache-2.0). `onnxruntime-gpu` (MIT) is the GPU build of onnxruntime, required by `catt_tashkeel`; it ran on CPU here.

Many packages (`gradio`, `fastapi`, `uvicorn`, `boto3`, the Google client libraries and others) are only there because `silma-tts` lists them and are not used by this program. A smaller install is possible but was not tried.

## The reference voice: what the project owner must record

ScarfeeOS has no voice yet. Someone must record one and keep these records with it (not in this repository if they contain personal data):

1. **Length:** one clip of 3 to 8 seconds, never more than 8.0 s (the program refuses longer). One clip per language is the safest start; whether an Arabic clip can speak English well was not tested.
2. **Quality:** one speaker, a quiet room, no music, echo or background voices, no clipping, natural pace, a few tenths of a second of silence at each end. WAV, PCM 16-bit, mono, 24 kHz if possible (other rates are converted by SILMA). These are project requirements, not SILMA rules.
3. **Transcript:** a UTF-8 text file with exactly the words spoken, in the language spoken, with Arabic as it is said. It must contain no religious text. Keep it with the WAV.
4. **Written consent from the speaker**, dated and signed, that says: their voice is recorded and will be cloned by an AI model; the cloned voice is shipped in ScarfeeOS and used to speak text to its users in the languages named; whether it may be used commercially and for how long; how to withdraw consent and what happens then. The SILMA card makes its voice-cloning terms a condition of use (consent, no fraud, no misinformation, disclosure that speech is AI-generated), so the consent must cover them and the product must tell users that the voice is synthetic.
5. **Record:** the speaker's name or id, the recording date, the device, the SHA-256 of the WAV and of the transcript, and who checked the transcript.

If the speaker is not the project owner, the owner also needs the speaker's rights in the recording itself, not only consent to the cloning.
