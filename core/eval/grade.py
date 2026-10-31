#!/usr/bin/env python3
"""Grades a results file against prompts.jsonl.

    python3 grade.py results/<label>.jsonl --today 2026-10-30 [--prompts prompts.jsonl]
    python3 grade.py results/pipeline.jsonl --pipeline --today 2026-10-30
    python3 grade.py --compare results/before.jsonl results/after.jsonl --today 2026-10-30

The results come from run_eval.ps1 (model replies), or with --pipeline from run_pipeline.py
(what `scarfeeos-core chat` printed). For each prompt with an expected tool it reports:
  tool  - a tool call with the expected name exists. A call the assistant holds back to ask
          the user (confirm the time, confirm before running, ask for a date and time) counts
          as a pass, and is also counted in its own column, held.
  args  - the identifier args match: name (case-insensitive), query, lang and attendees must
          equal the expected value (trimmed, case-insensitive; no translation). A list_files
          path matches when its last folder name does ("Documents" and "~/Documents"), or when it is
          an Arabic name the core maps to that folder (المستندات, ~/التنزيلات).
  strict - every expected arg except `time`, strictly as above, with free text included and
          the path compared in full, except those Arabic folder names. Paraphrases fail this on purpose.
  lenient - the same args, but free text (reminder `text`, meeting `title`) passes when it has
          the expected key words, and attendees ignore Arabic letter variants. Key words are
          compared after dropping diacritics and tatweel, folding alef/ya/ta marbuta/hamza
          variants and Arabic-Indic digits, dropping the definite article and the b-, l-, k-,
          w-, f- prefixes, comparing Arabic words by their consonant skeleton (a weak
          letter - alef, waw, ya, ha - is ignored, so اتصل matches الاتصال) and Latin words
          without a plural or -ing/-ed ending. Extra words and stop words are ignored. File
          names (`query`), paths, names, languages and times stay exact. Synonyms do not match.
  text  - the free-text args (reminder `text`, meeting `title`) are non-empty and in the same
          script as the user's prompt, Arabic or Latin. Any wording passes.
  time  - the `time` arg matches the expected description, resolved against --today. A time
          the assistant asked the user to confirm is graded as its candidate. A prompt the
          assistant said has no date or time fails.
strict and lenient are always printed side by side. Every reply is also checked for Cyrillic
or CJK characters (script leakage); the report lists the characters per prompt and totals them.
--compare grades two results files and lists the prompts whose grades changed.

Only the description forms found in prompts.jsonl are understood: "tomorrow HH:MM",
"<Weekday> HH:MM" and "tomorrow morning". Anything else is reported as unchecked.
A weekday means its next occurrence after --today. "morning" means 05:00 to 11:59.
The model's ISO 8601 time is compared as written; a UTC offset is ignored.
"""
import argparse
import json
import re
import sys
import unicodedata
from collections import Counter, OrderedDict
from datetime import date, datetime, timedelta
from pathlib import Path

WEEKDAYS = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday']
MORNING = (5 * 60, 12 * 60)
FREE_TEXT = ('text', 'title')

# (name, inclusive code point ranges)
LEAK_SCRIPTS = [
    ('Cyrillic', [(0x0400, 0x052F), (0x1C80, 0x1C8F), (0x2DE0, 0x2DFF), (0xA640, 0xA69F)]),
    ('CJK', [(0x2E80, 0x2FDF), (0x3000, 0x30FF), (0x3100, 0x312F), (0x3400, 0x4DBF),
             (0x4E00, 0x9FFF), (0xAC00, 0xD7AF), (0xF900, 0xFAFF), (0xFF00, 0xFFEF),
             (0x20000, 0x2FA1F)]),
]

DESCRIPTION = re.compile(
    r'^(tomorrow|' + '|'.join(WEEKDAYS) + r')\s+(?:(\d{1,2}):(\d{2})|(morning))$', re.IGNORECASE)


def resolve_description(description, today):
    """Returns (date, (first_minute, end_minute)) for a supported description, else None."""
    match = DESCRIPTION.match(description.strip()) if isinstance(description, str) else None
    if not match:
        return None
    day, hour, minute, morning = match.groups()
    day = day.lower()
    if day == 'tomorrow':
        when = today + timedelta(days=1)
    else:
        when = today + timedelta(days=(WEEKDAYS.index(day) - today.weekday()) % 7 or 7)
    if morning:
        return when, MORNING
    hour, minute = int(hour), int(minute)
    if hour > 23 or minute > 59:
        return None
    start = hour * 60 + minute
    return when, (start, start + 1)


def check_time(description, value, today):
    """Returns (status, detail); status is pass, fail or unchecked."""
    expected = resolve_description(description, today)
    if expected is None:
        return 'unchecked', f'unsupported time description {description!r}'
    when, (start, end) = expected
    if not isinstance(value, str):
        return 'fail', f'time is {value!r}, expected ISO 8601 for {description!r} ({when})'
    text = value.strip()
    try:
        got = datetime.fromisoformat(text[:-1] + '+00:00' if text[-1:] in 'Zz' else text)
    except ValueError:
        return 'fail', f'time {value!r} is not ISO 8601, expected {description!r} ({when})'
    minutes = got.hour * 60 + got.minute
    if got.date() == when and start <= minutes < end:
        return 'pass', ''
    return 'fail', f'time {value!r}, expected {description!r} ({when})'


def normalize(value):
    if isinstance(value, str):
        return value.strip().casefold()
    if isinstance(value, list):
        return sorted(normalize(item) for item in value)
    return value


def call_arguments(call):
    """Returns the arguments of a tool call as a dict, or None if they are unusable."""
    arguments = call.get('arguments')
    if isinstance(arguments, str):
        if not arguments.strip():
            return {}
        try:
            arguments = json.loads(arguments)
        except ValueError:
            return None
    if arguments is None:
        return {}
    return arguments if isinstance(arguments, dict) else None


def last_folder(path):
    parts = [part for part in re.split(r'[/\\]+', path.strip()) if part]
    return parts[-1].casefold() if parts else ''


# The Arabic names the core maps to a standard folder, copied from FOLDER_NAMES in
# StandardFolder::from_name (core/src/actions.rs), written without the leading ال. Only these
# forms are mirrored, not the leading words or glued letters the core also strips.
CORE_FOLDER_NAMES = {
    'documents': ['مستندات', 'وثائق', 'دوكيمنتس', 'دوكيومنتس'],
    'downloads': ['تنزيلات', 'تحميلات', 'داونلود', 'داونلودز', 'داونلودات', 'دونلود', 'دانلود', 'دونلودز'],
    'desktop': ['سطح المكتب', 'سطح مكتب', 'ديسكتوب'],
    'pictures': ['صور', 'بكتشرز', 'بيكتشرز'],
    'music': ['موسيقى', 'موسيقا', 'اغاني', 'ميوزك'],
    'videos': ['فيديو', 'فيديوهات', 'فيديوز', 'فديوهات', 'مقاطع فيديو', 'مقاطع الفيديو'],
}


def core_normalize(text):
    """Mirrors normalize in core/src/actions.rs."""
    text = ''.join(ch for ch in text if not '\u064b' <= ch <= '\u0652')
    text = text.translate({0x623: 'ا', 0x625: 'ا', 0x622: 'ا', 0x629: 'ه', 0x649: 'ي'})
    return ''.join(chr(ord(ch) - 0x0660 + 0x30) if '\u0660' <= ch <= '\u0669' else ch for ch in text).lower()


CORE_FOLDERS = {core_normalize(name): folder for folder, names in CORE_FOLDER_NAMES.items() for name in names}


def core_folder(path):
    """The standard folder the core maps a bare Arabic folder name, with or without ~/, to."""
    name = path.strip()
    if name.startswith('~'):
        name = name[1:].lstrip('/\\')
    name = core_normalize(name.rstrip('/\\').strip())
    return CORE_FOLDERS.get(name[2:] if name.startswith('ال') else name)


def maps_to_same_folder(got, want):
    folder = core_folder(got)
    return folder is not None and folder == last_folder(want)


def path_matches(got, want, compare):
    return compare(got, want) or maps_to_same_folder(got, want)


def arg_matches(key, got, want):
    if key == 'path' and isinstance(got, str) and isinstance(want, str):
        return path_matches(got, want, lambda g, w: last_folder(g) == last_folder(w))
    return normalize(got) == normalize(want)


def check_values(expected, arguments, skip, compare):
    if arguments is None:
        return 'fail', 'arguments are not a JSON object'
    wrong = []
    for key, want in expected.items():
        if key in skip:
            continue
        if key not in arguments:
            wrong.append(f'{key} missing, expected {want!r}')
        elif not compare(key, arguments[key], want):
            wrong.append(f'{key}={arguments[key]!r}, expected {want!r}')
    return ('fail', '; '.join(wrong)) if wrong else ('pass', '')


def strict_matches(key, got, want):
    if key == 'path' and isinstance(got, str) and isinstance(want, str):
        return path_matches(got, want, lambda g, w: normalize(g) == normalize(w))
    return normalize(got) == normalize(want)


def check_strict(expected, arguments):
    """Compares every expected arg except `time` strictly. Extra args are ignored."""
    return check_values(expected, arguments, ('time',), strict_matches)


def check_args(expected, arguments):
    """Compares the identifier args. Free text and `time` are graded separately."""
    return check_values(expected, arguments, ('time',) + FREE_TEXT, arg_matches)


ARABIC_MARKS = re.compile('[\u0610-\u061a\u064b-\u065f\u0670\u0640]')
ARABIC_LETTERS = str.maketrans({'\u0623': 'ا', '\u0625': 'ا', '\u0622': 'ا', '\u0671': 'ا', '\u0649': 'ي',
                                '\u0629': 'ه', '\u0624': 'و', '\u0626': 'ي', '\u06a9': 'ك', '\u06cc': 'ي'})
ARABIC_WORD = re.compile('[\u0600-\u06ff]')
ARABIC_PREFIX = re.compile('^[وف]?(?:ال|لل|[بكل]ال)')
WEAK_LETTERS = str.maketrans('', '', 'اويه')
STOP_WORDS = {'في', 'من', 'على', 'الى', 'عن', 'مع', 'ان', 'ما', 'the', 'a', 'an', 'to', 'of', 'for', 'and',
              'with', 'my', 'me', 'at', 'on', 'in'}


def fold_arabic(text):
    """Lower-cases and drops diacritics and tatweel; folds alef, ya, ta marbuta, hamza and digit variants."""
    text = ARABIC_MARKS.sub('', unicodedata.normalize('NFKC', text)).casefold().translate(ARABIC_LETTERS)
    return ''.join(str(unicodedata.digit(ch)) if unicodedata.category(ch) == 'Nd' else ch for ch in text)


def word_key(word):
    """A comparable key for a folded word: Arabic by consonant skeleton, Latin without common endings."""
    if ARABIC_WORD.search(word):
        stem = ARABIC_PREFIX.sub('', word)
        stem = stem if len(stem) >= 2 else word
        skeleton = stem.translate(WEAK_LETTERS)
        return skeleton if len(skeleton) >= 2 else stem
    for ending in ('ing', 'ed', 'es', 's'):
        if word.endswith(ending) and len(word) - len(ending) >= 3:
            return word[:-len(ending)]
    return word


def key_words(text):
    """The key of each word of text that is not a stop word (all words if every one is)."""
    words = re.findall(r'\w+', fold_arabic(text))
    keys = [word_key(w) for w in words if w not in STOP_WORDS]
    return keys or [word_key(w) for w in words]


def lenient_matches(key, got, want):
    if key in FREE_TEXT and isinstance(got, str) and isinstance(want, str):
        wanted, offered = key_words(want), set(key_words(got))
        return bool(wanted) and all(word in offered for word in wanted)
    if key == 'attendees' and isinstance(got, list) and isinstance(want, list):
        return all(isinstance(g, str) for g in got) and sorted(map(fold_arabic, got)) == sorted(map(fold_arabic, want))
    return arg_matches(key, got, want)


def check_lenient(expected, arguments):
    """Like check_strict, but free text matches on key words and attendees ignore letter variants."""
    return check_values(expected, arguments, ('time',), lenient_matches)


def dominant_scripts(text):
    """Returns the scripts, Arabic and/or Latin, that have the most letters in text (both on a tie)."""
    arabic = latin = 0
    for ch in text:
        if ch.isalpha():
            name = unicodedata.name(ch, '')
            arabic += name.startswith('ARABIC')
            latin += name.startswith('LATIN')
    return {'Arabic'} if arabic > latin else {'Latin'} if latin > arabic else {'Arabic', 'Latin'} if arabic else set()


def check_text(expected, arguments, prompt_text):
    """Free-text args must be non-empty and mostly in a script the user wrote in."""
    if arguments is None:
        return 'fail', 'arguments are not a JSON object'
    prompt = prompt_text if isinstance(prompt_text, str) else ''
    written = {s for ch in prompt if ch.isalpha()
               for s in ('Arabic', 'Latin') if unicodedata.name(ch, '').startswith(s.upper())}
    wrong = []
    for key in FREE_TEXT:
        if key not in expected:
            continue
        value = arguments.get(key)
        if not isinstance(value, str) or not value.strip():
            wrong.append(f'{key} is empty or missing')
            continue
        used = dominant_scripts(value)
        if not used:
            wrong.append(f'{key}={value!r} has no letters')
        elif written and not used & written:
            wrong.append(f"{key}={value!r} is {' and '.join(sorted(used))}, the prompt is {' and '.join(sorted(written))}")
    return ('fail', '; '.join(wrong)) if wrong else ('pass', '')


def leaked_characters(text):
    """Returns {script: [characters in order]} for the unexpected scripts found in text."""
    found = {}
    for name, ranges in LEAK_SCRIPTS:
        chars = [ch for ch in text if any(lo <= ord(ch) <= hi for lo, hi in ranges)]
        if chars:
            found[name] = chars
    return found


def find_leakage(text):
    return list(leaked_characters(text))


def describe_leakage(chars):
    """One line for a prompt: each script with its character count and the first distinct characters."""
    parts = []
    for name, found in chars.items():
        shown = ''.join(dict.fromkeys(found))
        parts.append(f"{name} {len(found)} {'character' if len(found) == 1 else 'characters'}: "
                     f"{shown[:10]}{'...' if len(shown) > 10 else ''}")
    return '; '.join(parts)


HELD_PREFIXES = [
    ('ask the user to confirm the time: ', 'clarify'),
    ('ask the user to confirm before running: ', 'confirm'),
    ('ask the user for a date and time: ', 'missing'),
]
LANGUAGE_CODES = {'English': 'en', 'Arabic': 'ar'}
ESCAPES = {'n': '\n', 'r': '\r', 't': '\t', '0': '\0', '\\': '\\', '"': '"', "'": "'"}


class DebugParser:
    """Reads the Rust `{:?}` text of a ToolCall: `Name { field: value, ... }`."""

    def __init__(self, text):
        self.text = text
        self.pos = 0

    def fail(self, what):
        raise ValueError(f'expected {what} at offset {self.pos} of {self.text!r}')

    def skip_space(self):
        while self.pos < len(self.text) and self.text[self.pos].isspace():
            self.pos += 1

    def take(self, char):
        self.skip_space()
        if self.text[self.pos:self.pos + 1] != char:
            self.fail(repr(char))
        self.pos += 1

    def peek(self, char):
        self.skip_space()
        return self.text[self.pos:self.pos + 1] == char

    def ident(self):
        self.skip_space()
        match = re.compile(r'[A-Za-z_][A-Za-z0-9_]*').match(self.text, self.pos)
        if not match:
            self.fail('a name')
        self.pos = match.end()
        return match.group()

    def string(self):
        self.take('"')
        out = []
        while True:
            if self.pos >= len(self.text):
                self.fail('a closing quote')
            char = self.text[self.pos]
            self.pos += 1
            if char == '"':
                return ''.join(out)
            if char != '\\':
                out.append(char)
                continue
            escape = self.text[self.pos:self.pos + 1]
            self.pos += 1
            if escape == 'u':
                match = re.compile(r'\{([0-9a-fA-F]{1,6})\}').match(self.text, self.pos)
                if not match:
                    self.fail('a \\u{...} escape')
                out.append(chr(int(match.group(1), 16)))
                self.pos = match.end()
            elif escape in ESCAPES:
                out.append(ESCAPES[escape])
            else:
                self.fail('a known escape')

    def value(self):
        if self.peek('"'):
            return self.string()
        if self.peek('['):
            self.take('[')
            items = []
            while not self.peek(']'):
                items.append(self.value())
                if not self.peek(']'):
                    self.take(',')
            self.take(']')
            return items
        name = self.ident()
        if name == 'None':
            return None
        if name == 'Some':
            self.take('(')
            inner = self.value()
            self.take(')')
            return inner
        return name

    def tool_call(self):
        """Returns (snake_case_name, {field: value})."""
        name = re.sub(r'(?<!^)(?=[A-Z])', '_', self.ident()).lower()
        fields = {}
        if self.peek('{'):
            self.take('{')
            while not self.peek('}'):
                key = self.ident()
                self.take(':')
                fields[key] = self.value()
                if not self.peek('}'):
                    self.take(',')
            self.take('}')
        self.skip_space()
        if self.pos != len(self.text):
            self.fail('the end of the call')
        return name, fields


def parse_outcome(outcome):
    """Turns the text `scarfeeos-core chat` printed into a results row without id or seconds.

    Returns reply, tool_calls, held (None, clarify, confirm or missing) and error.
    """
    text = outcome.strip() if isinstance(outcome, str) else ''
    row = {'reply': text, 'tool_calls': [], 'held': None, 'error': None}
    if text.startswith('error:') or not text:
        row['error'] = text or 'no output'
        return row
    prefix, held = next(((p, h) for p, h in HELD_PREFIXES if text.startswith(p)), ('run: ', None))
    if not text.startswith(prefix):
        return row
    try:
        name, fields = DebugParser(text[len(prefix):]).tool_call()
    except ValueError as exc:
        row['error'] = f'unreadable outcome: {exc}'
        return row
    if name == 'set_language' and fields.get('lang') in LANGUAGE_CODES:
        fields['lang'] = LANGUAGE_CODES[fields['lang']]
    row.update(tool_calls=[{'name': name, 'arguments': fields}], held=held)
    return row


def pipeline_results(rows):
    """Converts run_pipeline.py rows (id, outcome, seconds) to the results format."""
    return [dict(parse_outcome(row.get('outcome')), id=row['id']) for row in rows]


def grade_row(prompt, result, today):
    """Returns a dict with tool, held, args, strict, lenient, text, time, leakage and issues.

    leak_chars maps each leaked script to its character count. tool, args, strict, lenient, text and time are pass/fail/unchecked/na/None; held is True or False.
    """
    grade = {'id': prompt['id'], 'category': prompt['category'], 'tool': None, 'held': False,
             'args': None, 'strict': None, 'lenient': None, 'text': None, 'time': None, 'leakage': [],
             'leak_chars': {}, 'lenient_detail': '', 'issues': []}
    expected = prompt.get('tool')
    wants = {}
    if expected:
        wants = {'args': any(k not in ('time',) + FREE_TEXT for k in expected['args']),
                 'text': any(k in expected['args'] for k in FREE_TEXT),
                 'time': 'time' in expected['args']}

    def skip(tool):
        grade['tool'] = tool
        for key, wanted in wants.items():
            grade[key] = 'na' if wanted else None
        grade['strict'] = grade['lenient'] = 'na'

    if result is None:
        grade['issues'].append('no result row')
        if expected:
            skip('missing')
        return grade

    leaked = leaked_characters(result.get('reply') or '')
    grade['leakage'] = list(leaked)
    grade['leak_chars'] = {name: len(found) for name, found in leaked.items()}
    if leaked:
        grade['issues'].append('script leakage: ' + describe_leakage(leaked))
    if not expected:
        return grade

    if result.get('error'):
        skip('error')
        grade['issues'].append(f"run error: {result['error']}")
        return grade

    calls = [c for c in result.get('tool_calls') or [] if isinstance(c, dict)]
    match = next((c for c in calls if c.get('name') == expected['name']), None)
    if match is None:
        names = ', '.join(str(c.get('name')) for c in calls) or 'none'
        skip('fail')
        grade['issues'].append(f"tool: expected {expected['name']}, called {names}")
        return grade

    grade['tool'] = 'pass'
    held = result.get('held')
    grade['held'] = bool(held)
    arguments = call_arguments(match)
    grade['strict'], _ = check_strict(expected['args'], arguments)
    grade['lenient'], lenient_detail = check_lenient(expected['args'], arguments)
    if wants['args']:
        grade['args'], detail = check_args(expected['args'], arguments)
        if detail:
            grade['issues'].append('args: ' + detail)
    if wants['text']:
        grade['text'], detail = check_text(expected['args'], arguments, prompt.get('prompt'))
        if detail:
            grade['issues'].append('text: ' + detail)
    if grade['lenient'] == 'fail' and grade['args'] != 'fail' and grade['text'] != 'fail':
        grade['lenient_detail'] = lenient_detail
    if wants['time']:
        if held == 'missing':
            grade['time'] = 'fail'
            grade['issues'].append(f"time: asked the user for a date and time, expected {expected['args']['time']!r}")
        else:
            value = arguments.get('time') if arguments is not None else None
            grade['time'], detail = check_time(expected['args']['time'], value, today)
            if detail:
                grade['issues'].append(('time: ' if grade['time'] == 'fail' else 'note: ') + detail)
    return grade


def read_jsonl(path):
    with open(path, encoding='utf-8-sig') as handle:
        return [json.loads(line) for line in handle if line.strip()]


def grade_all(prompts, results, today):
    by_id = {row['id']: row for row in results}
    return [grade_row(p, by_id.get(p['id']), today) for p in prompts]


def ratio(counter):
    graded = counter['pass'] + counter['fail']
    return f"{counter['pass']}/{graded}" if graded else '-'


COLUMNS = ('tool', 'args', 'strict', 'lenient', 'text', 'time')


def leakage_summary(grades):
    scripts = Counter()
    for grade in grades:
        scripts.update(grade['leak_chars'])
    replies = sum(bool(g['leak_chars']) for g in grades)
    if not scripts:
        return f'Script leakage: none in {len(grades)} prompts.'
    detail = ', '.join(f'{name} {count}' for name, count in sorted(scripts.items()))
    return (f'Script leakage: {sum(scripts.values())} characters in {replies} of {len(grades)} replies ({detail}). '
            'Each is listed under Failures.')


def format_report(grades, today, unknown_ids=()):
    categories = OrderedDict()
    for grade in grades:
        entry = categories.setdefault(grade['category'], {
            'prompts': 0, 'leak': 0, 'held': 0, **{key: Counter() for key in COLUMNS}})
        entry['prompts'] += 1
        entry['leak'] += bool(grade['leakage'])
        entry['held'] += grade['held']
        for key in COLUMNS:
            if grade[key]:
                entry[key][grade[key]] += 1

    lines = [f'Graded against today = {today} ({WEEKDAYS[today.weekday()].capitalize()})', '',
             f"{'category':<12}{'prompts':>8}{'tool':>8}{'held':>6}{'args':>8}{'strict':>8}{'lenient':>9}"
             f"{'text':>8}{'time':>8}{'unchecked':>11}{'leakage':>9}"]
    for name, entry in categories.items():
        lines.append(f"{name:<12}{entry['prompts']:>8}{ratio(entry['tool']):>8}{entry['held']:>6}"
                     f"{ratio(entry['args']):>8}{ratio(entry['strict']):>8}{ratio(entry['lenient']):>9}"
                     f"{ratio(entry['text']):>8}{ratio(entry['time']):>8}{entry['time']['unchecked']:>11}{entry['leak']:>9}")
    lines.append('')
    lines.append('Columns are passed/graded. args, strict, lenient, text and time are not graded when no matching tool call exists.')
    lines.append('held: tool passes where the assistant asked the user first (included in tool).')
    lines.append('args: identifiers; text: reminder text and meeting title, script only.')
    lines.append('strict: all args exactly, a paraphrase fails. lenient: free text on key words, Arabic letter variants folded; '
                 'file names, paths and names stay exact.')
    totals = {key: Counter(g[key] for g in grades) for key in ('strict', 'lenient')}
    lines += ['', f"Scores over all categories: strict {ratio(totals['strict'])}, lenient {ratio(totals['lenient'])}",
              leakage_summary(grades)]

    failures = [g for g in grades if g['issues'] and any(not i.startswith('note: ') for i in g['issues'])]
    notes = [g for g in grades if any(i.startswith('note: ') for i in g['issues'])]
    lines += ['', f'Failures ({len(failures)}):']
    for grade in failures:
        for issue in grade['issues']:
            lines.append(f"  {grade['id']}  {issue}")
    if not failures:
        lines.append('  none')
    misses = [g for g in grades if g['lenient_detail']]
    lines += ['', f'Wording that fails even the lenient check ({len(misses)}):']
    lines += [f"  {g['id']}  {g['lenient_detail']}" for g in misses] or ['  none']
    if notes:
        lines += ['', 'Unchecked:']
        for grade in notes:
            for issue in grade['issues']:
                if issue.startswith('note: '):
                    lines.append(f"  {grade['id']}  {issue[6:]}")
    if unknown_ids:
        lines += ['', 'Result ids not in the prompts file (ignored): ' + ', '.join(unknown_ids)]
    return '\n'.join(lines)


COMPARED = ('tool', 'held', 'args', 'strict', 'lenient', 'text', 'time')


def answer_signature(row):
    if row is None:
        return None
    return json.dumps([row.get('reply'), row.get('tool_calls'), row.get('held'), row.get('error')],
                      sort_keys=True, ensure_ascii=False)


def compare_grades(before, after):
    """Returns (id, verdict, changes) for each prompt whose grades differ; verdict is improved, worse or mixed."""
    changed = []
    for old, new in zip(before, after):
        changes = [f'{key} {old[key]} -> {new[key]}' for key in COMPARED if old[key] != new[key]]
        old_chars, new_chars = sum(old['leak_chars'].values()), sum(new['leak_chars'].values())
        if old_chars != new_chars:
            changes.append(f'leaked characters {old_chars} -> {new_chars}')
        if not changes:
            continue
        better = sum(new[key] == 'pass' and old[key] != 'pass' for key in COMPARED if key != 'held') + (new_chars < old_chars)
        worse = sum(old[key] == 'pass' and new[key] != 'pass' for key in COMPARED if key != 'held') + (new_chars > old_chars)
        verdict = 'mixed' if better and worse else 'improved' if better else 'worse' if worse else 'changed'
        changed.append((old['id'], verdict, changes))
    return changed


def format_comparison(prompts, results_before, results_after, today, names):
    """Grades two result sets against the same prompts and lists the prompts whose grades changed."""
    before = grade_all(prompts, results_before, today)
    after = grade_all(prompts, results_after, today)
    changed = compare_grades(before, after)
    flagged = {item[0] for item in changed}
    old_rows = {row['id']: row for row in results_before}
    new_rows = {row['id']: row for row in results_after}
    reworded = [p['id'] for p in prompts if p['id'] not in flagged
                and answer_signature(old_rows.get(p['id'])) != answer_signature(new_rows.get(p['id']))]
    counts = Counter(verdict for _, verdict, _ in changed)
    detail = ', '.join(f'{counts[v]} {v}' for v in ('improved', 'worse', 'mixed', 'changed') if counts[v])
    lines = [f'Comparing {names[0]} (before) with {names[1]} (after), today = {today}', '',
             f"Prompts with changed grades: {len(changed)} of {len(prompts)}" + (f' ({detail})' if detail else ''),
             'Scores, passed/graded:']
    for key in COLUMNS:
        lines.append(f"  {key:<8}{ratio(Counter(g[key] for g in before)):>8} -> {ratio(Counter(g[key] for g in after))}")
    lines.append('  leaked characters  '
                 f"{sum(sum(g['leak_chars'].values()) for g in before)} -> "
                 f"{sum(sum(g['leak_chars'].values()) for g in after)}")
    lines += ['', 'Changed:']
    for prompt_id, verdict, changes in changed:
        lines.append(f"  {prompt_id}  {verdict}: " + '; '.join(changes))
    if not changed:
        lines.append('  none')
    if reworded:
        lines += ['', f'Answered differently with the same grades ({len(reworded)}): ' + ', '.join(reworded)]
    for label, rows, other in ((names[0], old_rows, new_rows), (names[1], new_rows, old_rows)):
        only = [p['id'] for p in prompts if p['id'] in rows and p['id'] not in other]
        if only:
            lines += ['', f'Only in {label}: ' + ', '.join(only)]
    return '\n'.join(lines)


def load_results(path, pipeline):
    rows = read_jsonl(path)
    return pipeline_results(rows) if pipeline else rows


def main(argv=None):
    here = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description='Grade a run_eval.ps1 or run_pipeline.py results file.')
    parser.add_argument('results', nargs='?', help='results/<label>.jsonl written by run_eval.ps1 or run_pipeline.py')
    parser.add_argument('--compare', nargs=2, metavar=('BEFORE', 'AFTER'),
                        help='grade two results files and list the prompts whose grades changed')
    parser.add_argument('--pipeline', action='store_true', help='the results are in the run_pipeline.py format')
    parser.add_argument('--prompts', default=str(here / 'prompts.jsonl'),
                        help='prompts file; use prompts-dialects.jsonl for the Egyptian and Gulf set')
    parser.add_argument('--today', required=True, help='run date, YYYY-MM-DD')
    args = parser.parse_args(argv)
    if bool(args.results) == bool(args.compare):
        parser.error('give one results file, or --compare BEFORE AFTER')
    try:
        today = date.fromisoformat(args.today)
    except ValueError:
        parser.error('--today must be YYYY-MM-DD')

    prompts = read_jsonl(args.prompts)
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8')
    if args.compare:
        before, after = (load_results(path, args.pipeline) for path in args.compare)
        names = [Path(path).name for path in args.compare]
        print(format_comparison(prompts, before, after, today, names))
        return 0
    results = load_results(args.results, args.pipeline)
    known = {p['id'] for p in prompts}
    unknown = [r['id'] for r in results if r.get('id') not in known]
    print(format_report(grade_all(prompts, results, today), today, unknown))
    return 0


if __name__ == '__main__':
    sys.exit(main())
