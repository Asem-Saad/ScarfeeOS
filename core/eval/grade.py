#!/usr/bin/env python3
"""Grades a results file written by run_eval.ps1 against prompts.jsonl.

    python3 grade.py results/<label>.jsonl --today 2026-10-30 [--prompts prompts.jsonl]

For each prompt with an expected tool it reports three separate checks:
  tool  - a tool call with the expected name exists
  args  - the other expected args match (trimmed, case-insensitive; no translation)
  time  - the `time` arg matches the expected description, resolved against --today
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
from collections import Counter, OrderedDict
from datetime import date, datetime, timedelta
from pathlib import Path

WEEKDAYS = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday']
MORNING = (5 * 60, 12 * 60)

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


def check_args(expected, arguments):
    """Compares every expected arg except `time`. Extra args are ignored."""
    if arguments is None:
        return 'fail', 'arguments are not a JSON object'
    wrong = []
    for key, want in expected.items():
        if key == 'time':
            continue
        if key not in arguments:
            wrong.append(f'{key} missing, expected {want!r}')
        elif normalize(arguments[key]) != normalize(want):
            wrong.append(f'{key}={arguments[key]!r}, expected {want!r}')
    return ('fail', '; '.join(wrong)) if wrong else ('pass', '')


def find_leakage(text):
    found = []
    for name, ranges in LEAK_SCRIPTS:
        if any(lo <= ord(ch) <= hi for ch in text for lo, hi in ranges):
            found.append(name)
    return found


def grade_row(prompt, result, today):
    """Returns a dict with tool, args, time (pass/fail/unchecked/na/None), leakage and issues."""
    grade = {'id': prompt['id'], 'category': prompt['category'],
             'tool': None, 'args': None, 'time': None, 'leakage': [], 'issues': []}
    expected = prompt.get('tool')
    if result is None:
        grade['issues'].append('no result row')
        if expected:
            grade.update(tool='missing', args='na', time='na' if 'time' in expected['args'] else None)
        return grade

    grade['leakage'] = find_leakage(result.get('reply') or '')
    if grade['leakage']:
        grade['issues'].append('script leakage: ' + ', '.join(grade['leakage']))
    if not expected:
        return grade

    wants_time = 'time' in expected['args']
    if result.get('error'):
        grade.update(tool='error', args='na', time='na' if wants_time else None)
        grade['issues'].append(f"run error: {result['error']}")
        return grade

    calls = [c for c in result.get('tool_calls') or [] if isinstance(c, dict)]
    match = next((c for c in calls if c.get('name') == expected['name']), None)
    if match is None:
        names = ', '.join(str(c.get('name')) for c in calls) or 'none'
        grade.update(tool='fail', args='na', time='na' if wants_time else None)
        grade['issues'].append(f"tool: expected {expected['name']}, called {names}")
        return grade

    grade['tool'] = 'pass'
    arguments = call_arguments(match)
    grade['args'], detail = check_args(expected['args'], arguments)
    if detail:
        grade['issues'].append('args: ' + detail)
    if wants_time:
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


def format_report(grades, today, unknown_ids=()):
    categories = OrderedDict()
    for grade in grades:
        entry = categories.setdefault(grade['category'], {
            'prompts': 0, 'leak': 0, 'tool': Counter(), 'args': Counter(), 'time': Counter()})
        entry['prompts'] += 1
        entry['leak'] += bool(grade['leakage'])
        for key in ('tool', 'args', 'time'):
            if grade[key]:
                entry[key][grade[key]] += 1

    lines = [f'Graded against today = {today} ({WEEKDAYS[today.weekday()].capitalize()})', '',
             f"{'category':<12}{'prompts':>8}{'tool':>8}{'args':>8}{'time':>8}{'unchecked':>11}{'leakage':>9}"]
    for name, entry in categories.items():
        lines.append(f"{name:<12}{entry['prompts']:>8}{ratio(entry['tool']):>8}{ratio(entry['args']):>8}"
                     f"{ratio(entry['time']):>8}{entry['time']['unchecked']:>11}{entry['leak']:>9}")
    lines.append('')
    lines.append('tool/args/time are passed/graded. args and time are not graded when no matching tool call exists.')

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
    parser = argparse.ArgumentParser(description='Grade a run_eval.ps1 results file.')
    parser.add_argument('results', help='results/<label>.jsonl written by run_eval.ps1')
    parser.add_argument('--prompts', default=str(here / 'prompts.jsonl'))
    parser.add_argument('--today', required=True, help='run date, YYYY-MM-DD')
    args = parser.parse_args(argv)
    try:
        today = date.fromisoformat(args.today)
    except ValueError:
        parser.error('--today must be YYYY-MM-DD')

    prompts = read_jsonl(args.prompts)
    results = read_jsonl(args.results)
    known = {p['id'] for p in prompts}
    unknown = [r['id'] for r in results if r.get('id') not in known]
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8')
    print(format_report(grade_all(prompts, results, today), today, unknown))
    return 0


if __name__ == '__main__':
    sys.exit(main())
