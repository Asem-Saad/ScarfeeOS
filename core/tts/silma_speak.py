#!/usr/bin/env python3
"""Speak sentences with SILMA TTS. The program that CommandSpeaker (core/src/tts.rs) runs.

    silma_speak.py OUT_WAV LANGUAGE REF_WAV REF_TEXT_FILE

Sentences come from stdin, one per line, as UTF-8. One WAV with all of them is
written to OUT_WAV. LANGUAGE is `en` or `ar`. REF_WAV is the voice to clone and
REF_TEXT_FILE holds exactly what is said in it. Nothing is downloaded: the model
files must already be in the local Hugging Face cache (see README.md).

Exit status: 0 done, 2 bad arguments or input, 3 refused, 4 missing model files,
5 the model failed.
"""

import hashlib
import importlib.util
import os
import shutil
import sys
import tempfile
import wave

LANGUAGES = ('en', 'ar')
MAX_REF_SECONDS = 8.0  # SILMA cuts longer audio and then transcribes it from the network
PAUSE_SECONDS = 0.3
MAX_REF_TEXT_CHARS = 400

# The sample that ships inside the silma-tts package. Its transcript is religious text.
REFUSED_REF_NAME = 'ar.ref.24k.wav'
REFUSED_REF_SHA256 = 'b6b88232c3b851a3d9833c66349c71b2527de346b467ccba944809bad776c7ae'

OFFLINE_ENV = ('HF_HUB_OFFLINE', 'TRANSFORMERS_OFFLINE', 'HF_DATASETS_OFFLINE')
# (repository, file) pairs that SilmaTTS() loads with the default settings.
WEIGHT_FILES = (
    ('silma-ai/silma-tts', 'model.pt'),
    ('silma-ai/silma-tts', 'vocab.txt'),
    ('charactr/vocos-mel-24khz', 'config.yaml'),
    ('charactr/vocos-mel-24khz', 'pytorch_model.bin'),
)
# catt_tashkeel keeps its own models inside the package and downloads them on first use.
CATT_MODEL_FILES = ('onnx_models/eo_model/encoder.onnx', 'onnx_models/eo_model/decoder.onnx')


class Fail(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def parse_args(argv):
    if len(argv) != 4:
        raise Fail(2, 'usage: silma_speak.py OUT_WAV LANGUAGE REF_WAV REF_TEXT_FILE (sentences on stdin)')
    out, language, ref_wav, ref_text_file = argv
    if language not in LANGUAGES:
        raise Fail(2, f'language must be one of {", ".join(LANGUAGES)}, not {language!r}')
    if not out:
        raise Fail(2, 'the output path is empty')
    return out, language, ref_wav, ref_text_file


def read_sentences(data):
    """Non-empty stripped lines of UTF-8 `data` (bytes)."""
    try:
        text = data.decode('utf-8-sig')
    except UnicodeDecodeError as e:
        raise Fail(2, f'stdin is not UTF-8: {e}')
    sentences = [line.strip() for line in text.splitlines()]
    sentences = [s for s in sentences if s]
    if not sentences:
        raise Fail(2, 'stdin has no sentences')
    return sentences


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            digest.update(chunk)
    return digest.hexdigest()


def check_reference(ref_wav, ref_text_file):
    """Returns the transcript. Refuses the bundled sample, a missing or long reference, an empty transcript."""
    for name in {os.path.basename(ref_wav), os.path.basename(os.path.realpath(ref_wav))}:
        if name.lower() == REFUSED_REF_NAME:
            raise Fail(3, f'{REFUSED_REF_NAME} is refused: its transcript is religious text. Use your own reference voice')
    try:
        if sha256_of(ref_wav) == REFUSED_REF_SHA256:
            raise Fail(3, f'{ref_wav} is a copy of {REFUSED_REF_NAME}, which is refused: its transcript is religious text')
        with wave.open(ref_wav, 'rb') as w:
            seconds = w.getnframes() / w.getframerate()
    except OSError as e:
        raise Fail(2, f'cannot read the reference WAV {ref_wav}: {e}')
    except (wave.Error, EOFError, ZeroDivisionError) as e:
        raise Fail(2, f'the reference {ref_wav} is not a usable PCM WAV file: {e}')
    if not 0 < seconds <= MAX_REF_SECONDS:
        raise Fail(2, f'the reference is {seconds:.1f} s; it must be longer than 0 and at most {MAX_REF_SECONDS:g} s')
    try:
        with open(ref_text_file, 'rb') as f:
            transcript = f.read().decode('utf-8-sig').strip()
    except (OSError, UnicodeDecodeError) as e:
        raise Fail(2, f'cannot read the reference transcript {ref_text_file}: {e}')
    if not transcript:
        raise Fail(2, f'the reference transcript {ref_text_file} is empty')
    if len(transcript) > MAX_REF_TEXT_CHARS:
        raise Fail(2, f'the reference transcript is longer than {MAX_REF_TEXT_CHARS} characters')
    return ' '.join(transcript.split())


def force_offline():
    for name in OFFLINE_ENV:
        os.environ[name] = '1'


def missing_weights():
    """The model files that SilmaTTS() would download: Hugging Face cache misses, then the tashkeel models."""
    try:
        from huggingface_hub import try_to_load_from_cache
    except ImportError as e:
        raise Fail(4, f'huggingface_hub is not installed: {e}. Install requirements.txt in a virtual environment')
    missing = []
    for repo, name in WEIGHT_FILES:
        found = try_to_load_from_cache(repo, name)
        if not isinstance(found, (str, os.PathLike)):
            missing.append(f'{repo}/{name}')
    spec = importlib.util.find_spec('catt_tashkeel')
    if spec is None or not spec.submodule_search_locations:
        missing.append('catt_tashkeel (package)')
    else:
        package = list(spec.submodule_search_locations)[0]
        for name in CATT_MODEL_FILES:
            if not os.path.isfile(os.path.join(package, *name.split('/'))):
                missing.append(f'catt_tashkeel/{name}')
    return missing


def load_model():
    missing = missing_weights()
    if missing:
        raise Fail(4, 'model files are not cached and this program never downloads: ' + ', '.join(missing)
                   + '. Fetch them once with the pre-fetch step in core/tts/README.md')
    try:
        from silma_tts.api import SilmaTTS
    except ImportError as e:
        raise Fail(4, f'silma_tts is not installed: {e}. Install requirements.txt in a virtual environment')
    return SilmaTTS()


def join_wavs(parts, out_path):
    """Writes the WAV files `parts` one after another, with a short pause between, into `out_path`."""
    first = None
    with wave.open(out_path, 'wb') as out:
        for i, part in enumerate(parts):
            with wave.open(part, 'rb') as w:
                params = w.getparams()[:3] + (0, 'NONE', 'not compressed')
                if first is None:
                    first = params
                    out.setparams(first)
                elif params != first:
                    raise Fail(5, 'the model wrote WAV files with different formats')
                if i:
                    pause = int(PAUSE_SECONDS * params[2])
                    out.writeframes(b'\0' * (pause * params[0] * params[1]))
                out.writeframes(w.readframes(w.getnframes()))


def speak(out_path, sentences, ref_wav, ref_text, model):
    scratch = tempfile.mkdtemp(prefix='silma-speak-')
    try:
        parts = []
        for i, sentence in enumerate(sentences):
            part = os.path.join(scratch, f'{i}.wav')
            try:
                model.infer(ref_file=ref_wav, ref_text=ref_text, gen_text=sentence, file_wave=part)
            except Exception as e:
                raise Fail(5, f'SILMA failed on sentence {i + 1}: {type(e).__name__}: {e}')
            if not os.path.isfile(part):
                raise Fail(5, f'SILMA wrote no audio for sentence {i + 1}')
            parts.append(part)
        try:
            join_wavs(parts, out_path)
        except (OSError, wave.Error) as e:
            raise Fail(5, f'cannot write {out_path}: {e}')
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def main(argv=None, stdin=None):
    argv = sys.argv[1:] if argv is None else argv
    stdin = sys.stdin.buffer if stdin is None else stdin
    try:
        out_path, _language, ref_wav, ref_text_file = parse_args(argv)
        ref_text = check_reference(ref_wav, ref_text_file)
        sentences = read_sentences(stdin.read())
        force_offline()
        speak(out_path, sentences, ref_wav, ref_text, load_model())
    except Fail as e:
        print(f'silma_speak: {e}', file=sys.stderr)
        return e.code
    return 0


if __name__ == '__main__':
    sys.exit(main())
