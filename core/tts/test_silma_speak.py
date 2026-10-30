import contextlib
import io
import os
import shutil
import sys
import tempfile
import types
import unittest
import wave
from unittest import mock

import silma_speak

RATE = 24000
REAL_MISSING_WEIGHTS = silma_speak.missing_weights


def write_wav(path, seconds, rate=RATE, frame=b'\x01\x00'):
    with wave.open(path, 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(frame * int(seconds * rate))


def frames(path):
    with wave.open(path, 'rb') as w:
        return w.getnframes(), w.getframerate()


class FakeSilmaTTS:
    """Stands in for silma_tts.api.SilmaTTS. Writes a tenth of a second of audio per call."""
    instances = []
    calls = []
    fail_on = None

    def __init__(self, *args, **kwargs):
        FakeSilmaTTS.instances.append(self)

    def infer(self, ref_file, ref_text, gen_text, file_wave=None, **kwargs):
        FakeSilmaTTS.calls.append({'ref_file': ref_file, 'ref_text': ref_text, 'gen_text': gen_text})
        if gen_text == FakeSilmaTTS.fail_on:
            raise RuntimeError('boom')
        write_wav(file_wave, 0.1)
        return None, RATE, None


def fake_modules():
    api = types.ModuleType('silma_tts.api')
    api.SilmaTTS = FakeSilmaTTS
    package = types.ModuleType('silma_tts')
    package.api = api
    return {'silma_tts': package, 'silma_tts.api': api}


class Case(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix='silma-test-')
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        self.out = os.path.join(self.dir, 'speech.wav')
        self.ref = self.path('voice.wav')
        self.text = self.path('voice.txt')
        write_wav(self.ref, 3)
        self.write(self.text, 'This is my own recorded sentence.\n')
        FakeSilmaTTS.instances, FakeSilmaTTS.calls, FakeSilmaTTS.fail_on = [], [], None
        patches = [
            mock.patch.dict(sys.modules, fake_modules()),
            mock.patch.object(silma_speak, 'missing_weights', return_value=[]),
            mock.patch.dict(os.environ, {}),
        ]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)

    def path(self, name):
        return os.path.join(self.dir, name)

    def write(self, path, text):
        with open(path, 'wb') as f:
            f.write(text.encode('utf-8'))

    def run_main(self, sentences='Hello there.\n', language='en', ref=None, text=None, out=None, stdin=None):
        argv = [out or self.out, language, ref or self.ref, text or self.text]
        data = stdin if stdin is not None else sentences.encode('utf-8')
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = silma_speak.main(argv, io.BytesIO(data))
        return code, err.getvalue()

    def assertRefused(self, code, err, code_wanted, word):
        self.assertEqual(code, code_wanted, err)
        self.assertIn(word, err)
        self.assertFalse(os.path.exists(self.out))
        self.assertEqual(FakeSilmaTTS.instances, [], 'the model must not load')


class Arguments(Case):
    def test_wrong_count_prints_usage(self):
        for argv in ([], [self.out, 'en', self.ref], [self.out, 'en', self.ref, self.text, 'extra']):
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                self.assertEqual(silma_speak.main(argv, io.BytesIO(b'Hi.\n')), 2)
            self.assertIn('usage', err.getvalue())

    def test_unknown_language(self):
        code, err = self.run_main(language='fr')
        self.assertRefused(code, err, 2, 'language')

    def test_both_languages_are_accepted(self):
        self.assertEqual(self.run_main(language='en')[0], 0)
        self.assertEqual(self.run_main(language='ar', sentences='مرحبا بك.\n')[0], 0)

    def test_parse_args_returns_the_four_values(self):
        self.assertEqual(silma_speak.parse_args(['o.wav', 'ar', 'r.wav', 'r.txt']), ('o.wav', 'ar', 'r.wav', 'r.txt'))


class Input(Case):
    def test_one_sentence_per_line_in_order(self):
        code, err = self.run_main(sentences='First one.\n\n  Second one.  \r\nمرحبا بك.\n')
        self.assertEqual(code, 0, err)
        self.assertEqual([c['gen_text'] for c in FakeSilmaTTS.calls], ['First one.', 'Second one.', 'مرحبا بك.'])

    def test_the_model_loads_once(self):
        self.run_main(sentences='A.\nB.\nC.\n')
        self.assertEqual(len(FakeSilmaTTS.instances), 1)

    def test_last_line_without_newline_and_bom(self):
        self.assertEqual(self.run_main(stdin=b'\xef\xbb\xbfOnly one.')[0], 0)
        self.assertEqual([c['gen_text'] for c in FakeSilmaTTS.calls], ['Only one.'])

    def test_read_sentences(self):
        self.assertEqual(silma_speak.read_sentences('a\n\n b \n'.encode()), ['a', 'b'])

    def test_empty_stdin(self):
        for data in (b'', b'\n  \r\n'):
            code, err = self.run_main(stdin=data)
            self.assertRefused(code, err, 2, 'no sentences')

    def test_stdin_not_utf8(self):
        code, err = self.run_main(stdin=b'\xff\xfe bad\n')
        self.assertRefused(code, err, 2, 'UTF-8')

    def test_text_is_never_taken_from_the_arguments(self):
        self.run_main(sentences='$(touch pwned); "quoted" `x`\n')
        self.assertEqual(FakeSilmaTTS.calls[0]['gen_text'], '$(touch pwned); "quoted" `x`')


class Reference(Case):
    def test_reference_text_and_wav_reach_the_model(self):
        self.write(self.text, '﻿  My own\n sentence.  \n')
        self.assertEqual(self.run_main()[0], 0)
        self.assertEqual(FakeSilmaTTS.calls[0]['ref_text'], 'My own sentence.')
        self.assertEqual(FakeSilmaTTS.calls[0]['ref_file'], self.ref)

    def test_bundled_sample_is_refused_by_name(self):
        for name in ('ar.ref.24k.wav', 'AR.REF.24K.WAV'):
            ref = self.path(name)
            write_wav(ref, 3)
            code, err = self.run_main(ref=ref)
            self.assertRefused(code, err, 3, 'religious')

    def test_bundled_sample_is_refused_through_a_link(self):
        bundled = self.path('ar.ref.24k.wav')
        write_wav(bundled, 3)
        link = self.path('my-voice.wav')
        try:
            os.symlink(bundled, link)
        except (OSError, NotImplementedError):
            self.skipTest('no symbolic links here')
        code, err = self.run_main(ref=link)
        self.assertRefused(code, err, 3, 'religious')

    def test_bundled_sample_is_refused_when_renamed(self):
        with mock.patch.object(silma_speak, 'REFUSED_REF_SHA256', silma_speak.sha256_of(self.ref)):
            code, err = self.run_main()
        self.assertRefused(code, err, 3, 'religious')

    def test_missing_reference(self):
        code, err = self.run_main(ref=self.path('nope.wav'))
        self.assertRefused(code, err, 2, 'cannot read')

    def test_reference_is_not_a_wav(self):
        self.write(self.ref, 'not audio')
        code, err = self.run_main()
        self.assertRefused(code, err, 2, 'not a usable')

    def test_reference_too_long(self):
        write_wav(self.ref, 9)
        code, err = self.run_main()
        self.assertRefused(code, err, 2, 'at most')

    def test_reference_of_exactly_the_limit_is_accepted(self):
        write_wav(self.ref, silma_speak.MAX_REF_SECONDS)
        self.assertEqual(self.run_main()[0], 0)

    def test_reference_with_no_audio(self):
        write_wav(self.ref, 0)
        code, err = self.run_main()
        self.assertRefused(code, err, 2, 'at most')

    def test_missing_transcript(self):
        code, err = self.run_main(text=self.path('nope.txt'))
        self.assertRefused(code, err, 2, 'transcript')

    def test_empty_transcript(self):
        self.write(self.text, ' \n')
        code, err = self.run_main()
        self.assertRefused(code, err, 2, 'empty')

    def test_transcript_not_utf8(self):
        with open(self.text, 'wb') as f:
            f.write(b'\xff\xfe')
        code, err = self.run_main()
        self.assertRefused(code, err, 2, 'transcript')

    def test_transcript_too_long(self):
        self.write(self.text, 'x' * (silma_speak.MAX_REF_TEXT_CHARS + 1))
        code, err = self.run_main()
        self.assertRefused(code, err, 2, 'longer than')


class Offline(Case):
    def test_network_is_switched_off_before_the_model_loads(self):
        seen = {}
        original = FakeSilmaTTS.__init__

        def spy(model, *args, **kwargs):
            seen.update({name: os.environ.get(name) for name in silma_speak.OFFLINE_ENV})
            original(model, *args, **kwargs)

        os.environ['HF_HUB_OFFLINE'] = '0'
        with mock.patch.object(FakeSilmaTTS, '__init__', spy):
            self.assertEqual(self.run_main()[0], 0)
        self.assertEqual(seen, {name: '1' for name in silma_speak.OFFLINE_ENV})

    def test_uncached_weights_fail_clearly_without_loading(self):
        with mock.patch.object(silma_speak, 'missing_weights', return_value=['silma-ai/silma-tts/model.pt']):
            code, err = self.run_main()
        self.assertRefused(code, err, 4, 'never downloads')
        self.assertIn('model.pt', err)
        self.assertIn('README', err)

    def fake_cache(self, with_catt):
        hub = types.ModuleType('huggingface_hub')
        hub.try_to_load_from_cache = lambda repo, name: '/cache/x' if name == 'model.pt' else None
        package = self.path('catt_tashkeel')
        if with_catt:
            os.makedirs(os.path.join(package, 'onnx_models', 'eo_model'))
            for name in silma_speak.CATT_MODEL_FILES:
                open(os.path.join(package, *name.split('/')), 'wb').close()
        else:
            os.makedirs(package)
        spec = types.SimpleNamespace(submodule_search_locations=[package])
        return mock.patch.dict(sys.modules, {'huggingface_hub': hub}), \
            mock.patch('importlib.util.find_spec', return_value=spec)

    def test_missing_weights_asks_the_hugging_face_cache(self):
        hub_patch, spec_patch = self.fake_cache(with_catt=True)
        with hub_patch, spec_patch:
            missing = REAL_MISSING_WEIGHTS()
        self.assertEqual(missing, ['silma-ai/silma-tts/vocab.txt', 'charactr/vocos-mel-24khz/config.yaml',
                                   'charactr/vocos-mel-24khz/pytorch_model.bin'])

    def test_missing_weights_asks_for_the_tashkeel_models(self):
        hub_patch, spec_patch = self.fake_cache(with_catt=False)
        with hub_patch, spec_patch:
            missing = REAL_MISSING_WEIGHTS()
        self.assertEqual(missing[-2:], ['catt_tashkeel/onnx_models/eo_model/encoder.onnx',
                                        'catt_tashkeel/onnx_models/eo_model/decoder.onnx'])

    def test_silma_not_installed(self):
        with mock.patch.dict(sys.modules, {'silma_tts': None, 'silma_tts.api': None}):
            code, err = self.run_main()
        self.assertRefused(code, err, 4, 'not installed')


class EncodecStub(Case):
    def test_stub_is_registered_when_encodec_is_absent(self):
        with mock.patch.dict(sys.modules, {}), mock.patch.object(silma_speak.importlib.util, 'find_spec', return_value=None):
            sys.modules.pop('encodec', None)
            silma_speak.stub_encodec()
            from encodec import EncodecModel
            with self.assertRaises(ImportError):
                EncodecModel.encodec_model_24khz(pretrained=True)
            with self.assertRaises(ImportError):
                EncodecModel()

    def test_installed_encodec_is_left_alone(self):
        with mock.patch.dict(sys.modules, {}), \
                mock.patch.object(silma_speak.importlib.util, 'find_spec', return_value=object()):
            sys.modules.pop('encodec', None)
            silma_speak.stub_encodec()
            self.assertNotIn('encodec', sys.modules)

    def test_stub_survives_a_second_call_and_a_spec_lookup(self):
        with mock.patch.dict(sys.modules, {}):
            sys.modules.pop('encodec', None)
            silma_speak.stub_encodec()
            first = sys.modules['encodec']
            silma_speak.stub_encodec()
            self.assertIs(sys.modules['encodec'], first)
            self.assertIsNotNone(silma_speak.importlib.util.find_spec('encodec'))

    def test_the_model_load_installs_the_stub(self):
        with mock.patch.object(silma_speak, 'stub_encodec') as stub:
            self.assertEqual(self.run_main()[0], 0)
        stub.assert_called_once_with()


class Output(Case):
    def test_one_wav_with_every_sentence_and_pauses(self):
        self.assertEqual(self.run_main(sentences='A.\nB.\nC.\n')[0], 0)
        pause = int(silma_speak.PAUSE_SECONDS * RATE)
        self.assertEqual(frames(self.out), (3 * int(0.1 * RATE) + 2 * pause, RATE))

    def test_writes_into_the_file_that_already_exists(self):
        open(self.out, 'wb').close()
        before = os.stat(self.out).st_ino
        self.assertEqual(self.run_main()[0], 0)
        self.assertEqual(os.stat(self.out).st_ino, before)
        self.assertGreater(frames(self.out)[0], 0)

    def test_a_failing_sentence_stops_with_status_5(self):
        FakeSilmaTTS.fail_on = 'B.'
        code, err = self.run_main(sentences='A.\nB.\nC.\n')
        self.assertEqual(code, 5)
        self.assertIn('sentence 2', err)
        self.assertIn('boom', err)
        self.assertFalse(os.path.exists(self.out))
        self.assertEqual(len(FakeSilmaTTS.calls), 2)

    def test_no_audio_from_the_model(self):
        with mock.patch.object(FakeSilmaTTS, 'infer', lambda *a, **k: (None, RATE, None)):
            code, err = self.run_main()
        self.assertEqual(code, 5)
        self.assertIn('no audio', err)

    def test_unwritable_output(self):
        code, err = self.run_main(out=self.path('missing-dir/speech.wav'))
        self.assertEqual(code, 5)
        self.assertIn('cannot write', err)

    def test_scratch_files_are_removed(self):
        scratch = []
        original = tempfile.mkdtemp

        def spy(*args, **kwargs):
            scratch.append(original(*args, **kwargs))
            return scratch[-1]

        with mock.patch.object(tempfile, 'mkdtemp', spy):
            self.run_main()
            FakeSilmaTTS.fail_on = 'A.'
            self.run_main(sentences='A.\n')
        self.assertEqual(len(scratch), 2)
        self.assertFalse(any(os.path.exists(p) for p in scratch))

    def test_mixed_formats_are_an_error(self):
        parts = [self.path('a.wav'), self.path('b.wav')]
        write_wav(parts[0], 0.1)
        write_wav(parts[1], 0.1, rate=16000)
        with self.assertRaises(silma_speak.Fail) as caught:
            silma_speak.join_wavs(parts, self.out)
        self.assertEqual(caught.exception.code, 5)


if __name__ == '__main__':
    unittest.main()
