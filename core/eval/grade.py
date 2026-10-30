#!/usr/bin/env python3
"""Grades a results file against prompts.jsonl.

    python3 grade.py results/<label>.jsonl --today 2026-10-30 [--prompts prompts.jsonl]
    python3 grade.py results/pipeline.jsonl --pipeline --today 2026-10-30

The results come from run_eval.ps1 (model replies), or with --pipeline from run_pipeline.py
(what `scarfeeos-core chat` printed). For each prompt with an expected tool it reports:
  tool  - a tool call with the expected name exists. A call the assistant holds back to ask
          the user (confirm the time, confirm before running, ask for a date and time) counts
          as a pass, and is also counted in its own column, held.
  args  - the identifier args match: name (case-insensitive), query, lang and attendees must
          equal the expected value (trimmed, case-insensitive; no translation). A list_files
          path matches when its last folder name does ("Documents" and "~/Documents").
  exact - every expected arg except `time`, strictly as above, with free text included and
          the path compared in full. Paraphrases fail this on purpose; see text.
  text  - the free-text args (reminder `text`, meeting `title`) are non-empty and in the same
          script as the user's prompt, Arabic or Latin. Any wording passes.
  time  - the `time` arg matches the expected description, resolved against --today. A time
          the assistant asked the user to confirm is graded as its candidate. A prompt the
          assistant said has no date or time fails.
Every reply is also checked for Cyrillic or CJK characters (script leakage).

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


def arg_matches(key, got, want):
    if key == 'path' and isinstance(got, str) and isinstance(want, str):
        return last_folder(got) == last_folder(want)
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


def check_exact(expected, arguments):
    """Compares every expected arg except `time` strictly. Extra args are ignored."""
    return check_values(expected, arguments, ('time',), lambda key, got, want: normalize(got) == normalize(want))


def check_args(expected, arguments):
    """Compares the identifier args. Free text and `time` are graded separately."""
    return check_values(expected, arguments, ('time',) + FREE_TEXT, arg_matches)


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


def find_leakage(text):
    found = []
    for name, ranges in LEAK_SCRIPTS:
        if any(lo <= ord(ch) <= hi for ch in text for lo, hi in ranges):
            found.append(name)
    return found


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
    """Returns a dict with tool, held, args, exact, text, time, leakage and issues.

    tool, args, exact, text and time are pass/fail/unchecked/na/None; held is True or False.
    """
    grade = {'id': prompt['id'], 'category': prompt['category'], 'tool': None, 'held': False,
             'args': None, 'exact': None, 'text': None, 'time': None, 'leakage': [], 'issues': []}
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
        grade['exact'] = 'na'

    if result is None:
        grade['issues'].append('no result row')
        if expected:
            skip('missing')
        return grade

    grade['leakage'] = find_leakage(result.get('reply') or '')
    if grade['leakage']:
        grade['issues'].append('script leakage: ' + ', '.join(grade['leakage']))
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
    grade['exact'], _ = check_exact(expected['args'], arguments)
    if wants['args']:
        grade['args'], detail = check_args(expected['args'], arguments)
        if detail:
            grade['issues'].append('args: ' + detail)
    if wants['text']:
        grade['text'], detail = check_text(expected['args'], arguments, prompt.get('prompt'))
        if detail:
            grade['issues'].append('text: ' + detail)
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


COLUMNS = ('tool', 'args', 'exact', 'text', 'time')


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
             f"{'category':<12}{'prompts':>8}{'tool':>8}{'held':>6}{'args':>8}{'exact':>8}{'text':>8}"
             f"{'time':>8}{'unchecked':>11}{'leakage':>9}"]
    for name, entry in categories.items():
        lines.append(f"{name:<12}{entry['prompts']:>8}{ratio(entry['tool']):>8}{entry['held']:>6}"
                     f"{ratio(entry['args']):>8}{ratio(entry['exact']):>8}{ratio(entry['text']):>8}"
                     f"{ratio(entry['time']):>8}{entry['time']['unchecked']:>11}{entry['leak']:>9}")
    lines.append('')
    lines.append('Columns are passed/graded. args, exact, text and time are not graded when no matching tool call exists.')
    lines.append('held: tool passes where the assistant asked the user first (included in tool).')
    lines.append('args: identifiers; exact: all args strictly, paraphrase fails; text: reminder text and meeting title.')

    failures = [g for g in grades if g['issues'] and any(not i.startswith('note: ') for i in g['issues'])]
    notes = [g for g in grades if any(i.startswith('note: ') for i in g['issues'])]
    lines += ['', f'Failures ({len(failures)}):']
    for grade in failures:
        for issue in grade['issues']:
            lines.append(f"  {grade['id']}  {issue}")
    if not failures:
        lines.append('  none')
    if notes:
        lines += ['', 'Unchecked:']
        for grade in notes:
            for issue in grade['issues']:
                if issue.startswith('note: '):
                    lines.append(f"  {grade['id']}  {issue[6:]}")
    if unknown_ids:
        lines += ['', 'Result ids not in the prompts file (ignored): ' + ', '.join(unknown_ids)]
    return '\n'.join(lines)


def main(argv=None):
    here = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description='Grade a run_eval.ps1 or run_pipeline.py results file.')
    parser.add_argument('results', help='results/<label>.jsonl written by run_eval.ps1 or run_pipeline.py')
    parser.add_argument('--pipeline', action='store_true', help='the results are in the run_pipeline.py format')
    parser.add_argument('--prompts', default=str(here / 'prompts.jsonl'),
                        help='prompts file; use prompts-dialects.jsonl for the Egyptian and Gulf set')
    parser.add_argument('--today', required=True, help='run date, YYYY-MM-DD')
    args = parser.parse_args(argv)
    try:
        today = date.fromisoformat(args.today)
    except ValueError:
        parser.error('--today must be YYYY-MM-DD')

    prompts = read_jsonl(args.prompts)
    results = read_jsonl(args.results)
    if args.pipeline:
        results = pipeline_results(results)
    known = {p['id'] for p in prompts}
    unknown = [r['id'] for r in results if r.get('id') not in known]
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8')
    print(format_report(grade_all(prompts, results, today), today, unknown))
    return 0


if __name__ == '__main__':
    sys.exit(main())
