import json
import os
import re
import stat
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from datetime import date
from io import StringIO
from pathlib import Path

import grade
import run_pipeline

TODAY = date(2026, 10, 30)  # a Friday
PROMPTS = Path(__file__).resolve().parent / 'prompts.jsonl'


def reminder_prompt(time='tomorrow 10:00'):
    return {'id': 'command-007', 'category': 'command', 'prompt': 'x',
            'tool': {'name': 'create_reminder', 'args': {'text': 'call the bank', 'time': time}}}


def result(calls=None, reply='ok', error=None):
    return {'id': 'command-007', 'category': 'command', 'reply': reply,
            'tool_calls': calls or [], 'error': error}


def call(tool, /, **arguments):
    return {'name': tool, 'arguments': json.dumps(arguments, ensure_ascii=False)}


class ResolveDescriptionTests(unittest.TestCase):
    def test_tomorrow_time(self):
        self.assertEqual(grade.resolve_description('tomorrow 10:00', TODAY),
                         (date(2026, 10, 31), (600, 601)))

    def test_weekday_is_next_occurrence(self):
        self.assertEqual(grade.resolve_description('Sunday 14:00', TODAY),
                         (date(2026, 11, 1), (840, 841)))
        self.assertEqual(grade.resolve_description('Friday 09:30', TODAY)[0], date(2026, 11, 6))

    def test_tomorrow_morning(self):
        self.assertEqual(grade.resolve_description('tomorrow morning', TODAY),
                         (date(2026, 10, 31), grade.MORNING))

    def test_other_forms_are_unsupported(self):
        for text in ['next week', 'tonight at 8', 'in 2 hours', 'tomorrow evening', 'tomorrow 25:00',
                     'غدا 10:00', '', None]:
            self.assertIsNone(grade.resolve_description(text, TODAY), text)


class CheckTimeTests(unittest.TestCase):
    def status(self, description, value):
        return grade.check_time(description, value, TODAY)[0]

    def test_exact_time(self):
        self.assertEqual(self.status('tomorrow 10:00', '2026-10-31T10:00:00'), 'pass')
        self.assertEqual(self.status('tomorrow 10:00', '2026-10-31T10:00:00+03:00'), 'pass')
        self.assertEqual(self.status('tomorrow 10:00', '2026-10-31T10:00:00Z'), 'pass')
        self.assertEqual(self.status('tomorrow 10:00', ' 2026-10-31 10:00 '), 'pass')

    def test_wrong_day_or_time(self):
        self.assertEqual(self.status('tomorrow 10:00', '2026-10-30T10:00:00'), 'fail')
        self.assertEqual(self.status('tomorrow 10:00', '2026-10-31T22:00:00'), 'fail')
        self.assertEqual(self.status('tomorrow 10:00', '2026-10-31T10:01:00'), 'fail')
        self.assertEqual(self.status('Sunday 14:00', '2026-11-08T14:00:00'), 'fail')
        self.assertEqual(self.status('Sunday 14:00', '2026-11-01T14:00:00'), 'pass')

    def test_not_iso_or_missing(self):
        self.assertEqual(self.status('tomorrow 10:00', 'tomorrow 10:00'), 'fail')
        self.assertEqual(self.status('tomorrow 10:00', ''), 'fail')
        self.assertEqual(self.status('tomorrow 10:00', None), 'fail')

    def test_morning_window(self):
        self.assertEqual(self.status('tomorrow morning', '2026-10-31T08:00:00'), 'pass')
        self.assertEqual(self.status('tomorrow morning', '2026-10-31T05:00:00'), 'pass')
        self.assertEqual(self.status('tomorrow morning', '2026-10-31T11:59:00'), 'pass')
        self.assertEqual(self.status('tomorrow morning', '2026-10-31T12:00:00'), 'fail')
        self.assertEqual(self.status('tomorrow morning', '2026-10-31T04:59:00'), 'fail')
        self.assertEqual(self.status('tomorrow morning', '2026-10-30T08:00:00'), 'fail')

    def test_unsupported_description_is_unchecked(self):
        self.assertEqual(self.status('next week', '2026-11-06T10:00:00'), 'unchecked')


class LeakageTests(unittest.TestCase):
    def test_detects_scripts(self):
        self.assertEqual(grade.find_leakage('привет'), ['Cyrillic'])
        self.assertEqual(grade.find_leakage('好的，我来打开'), ['CJK'])
        self.assertEqual(grade.find_leakage('こんにちは'), ['CJK'])
        self.assertEqual(grade.find_leakage('hello مرحبا Привет 你好'), ['Cyrillic', 'CJK'])

    def test_clean_text(self):
        self.assertEqual(grade.find_leakage('Opening the calculator. فتحت الآلة الحاسبة ١٢٣ «»'), [])
        self.assertEqual(grade.find_leakage(''), [])


class GradeRowTests(unittest.TestCase):
    def grade(self, prompt, res):
        return grade.grade_row(prompt, res, TODAY)

    def test_all_pass(self):
        res = result([call('create_reminder', text='  Call The Bank ', time='2026-10-31T10:00:00')])
        g = self.grade(reminder_prompt(), res)
        self.assertEqual((g['tool'], g['text'], g['exact'], g['time'], g['issues']),
                         ('pass', 'pass', 'pass', 'pass', []))
        self.assertEqual((g['args'], g['held']), (None, False))

    def test_extra_args_and_dict_arguments_are_accepted(self):
        res = result([{'name': 'create_reminder',
                       'arguments': {'text': 'call the bank', 'time': '2026-10-31T10:00:00', 'x': 1}}])
        g = self.grade(reminder_prompt(), res)
        self.assertEqual((g['tool'], g['text'], g['exact'], g['time']), ('pass', 'pass', 'pass', 'pass'))

    def test_checks_are_reported_separately(self):
        res = result([call('create_reminder', text='call the dentist', time='2026-10-31T10:00:00')])
        g = self.grade(reminder_prompt(), res)
        self.assertEqual((g['tool'], g['text'], g['exact'], g['time']), ('pass', 'pass', 'fail', 'pass'))
        self.assertEqual(g['issues'], [])
        res = result([call('create_reminder', text='call the bank', time='2026-10-31T11:00:00')])
        g = self.grade(reminder_prompt(), res)
        self.assertEqual((g['tool'], g['text'], g['exact'], g['time']), ('pass', 'pass', 'pass', 'fail'))

    def test_languages_are_not_interchangeable(self):
        prompt = {'id': 'command-002', 'category': 'command', 'prompt': 'x',
                  'tool': {'name': 'open_app', 'args': {'name': 'calculator'}}}
        res = {'id': 'command-002', 'reply': '', 'tool_calls': [call('open_app', name='الآلة الحاسبة')]}
        self.assertEqual(self.grade(prompt, res)['args'], 'fail')
        res['tool_calls'] = [call('open_app', name='Calculator')]
        g = self.grade(prompt, res)
        self.assertEqual((g['args'], g['exact']), ('pass', 'pass'))

    def test_attendees_ignore_order_and_case(self):
        prompt = {'id': 'command-009', 'category': 'command', 'prompt': 'x',
                  'tool': {'name': 'schedule_meeting', 'args': {
                      'title': 'Project Review', 'time': 'Sunday 14:00', 'attendees': ['Sara', 'Omar']}}}
        args = dict(title='project review', time='2026-11-01T14:00:00', attendees=['omar', 'SARA'])
        g = self.grade(prompt, {'id': 'command-009', 'tool_calls': [call('schedule_meeting', **args)]})
        self.assertEqual((g['tool'], g['args'], g['text'], g['exact'], g['time']),
                         ('pass', 'pass', 'pass', 'pass', 'pass'))
        args['attendees'] = ['Sara']
        g = self.grade(prompt, {'id': 'command-009', 'tool_calls': [call('schedule_meeting', **args)]})
        self.assertEqual((g['args'], g['exact']), ('fail', 'fail'))

    def test_missing_arg_and_bad_json(self):
        g = self.grade(reminder_prompt(), result([call('create_reminder', time='2026-10-31T10:00:00')]))
        self.assertEqual((g['text'], g['exact'], g['time']), ('fail', 'fail', 'pass'))
        g = self.grade(reminder_prompt(), result([{'name': 'create_reminder', 'arguments': '{oops'}]))
        self.assertEqual((g['tool'], g['text'], g['exact'], g['time']), ('pass', 'fail', 'fail', 'fail'))

    def test_wrong_or_no_tool(self):
        g = self.grade(reminder_prompt(), result([call('open_app', name='x')]))
        self.assertEqual((g['tool'], g['text'], g['exact'], g['time']), ('fail', 'na', 'na', 'na'))
        g = self.grade(reminder_prompt(), result([]))
        self.assertEqual(g['tool'], 'fail')
        self.assertIn('called none', g['issues'][0])

    def test_matching_call_is_found_among_several(self):
        res = result([call('open_app', name='x'),
                      call('create_reminder', text='call the bank', time='2026-10-31T10:00:00')])
        self.assertEqual(self.grade(reminder_prompt(), res)['tool'], 'pass')

    def test_unchecked_time_description(self):
        res = result([call('create_reminder', text='call the bank', time='2026-11-06T10:00:00')])
        g = self.grade(reminder_prompt('next week'), res)
        self.assertEqual((g['tool'], g['text'], g['time']), ('pass', 'pass', 'unchecked'))

    def test_prompt_without_tool_only_checks_leakage(self):
        prompt = {'id': 'qa-001', 'category': 'qa', 'prompt': 'x'}
        g = self.grade(prompt, {'id': 'qa-001', 'reply': 'Ответ', 'tool_calls': []})
        self.assertEqual((g['tool'], g['args'], g['exact'], g['text'], g['time'], g['leakage']),
                         (None, None, None, None, None, ['Cyrillic']))
        self.assertEqual(self.grade(prompt, {'id': 'qa-001', 'reply': None})['leakage'], [])

    def test_leakage_is_checked_on_tool_prompts_too(self):
        res = result([call('create_reminder', text='call the bank', time='2026-10-31T10:00:00')], reply='好的')
        g = self.grade(reminder_prompt(), res)
        self.assertEqual((g['tool'], g['leakage']), ('pass', ['CJK']))

    def test_run_error_and_missing_row(self):
        g = self.grade(reminder_prompt(), result(error='timeout'))
        self.assertEqual((g['tool'], g['text'], g['exact'], g['time']), ('error', 'na', 'na', 'na'))
        g = self.grade(reminder_prompt(), None)
        self.assertEqual((g['tool'], g['text'], g['exact'], g['time']), ('missing', 'na', 'na', 'na'))


class TextCheckTests(unittest.TestCase):
    def text(self, prompt, args, **arguments):
        return grade.check_text(args, arguments, prompt)[0]

    def test_paraphrase_passes_in_the_prompt_script(self):
        self.assertEqual(self.text('Remind me to call the bank tomorrow', {'text': 'call the bank'},
                                   text='phone the branch'), 'pass')
        self.assertEqual(self.text('ذكّرني بالاتصال بالبنك غدًا', {'text': 'الاتصال بالبنك'},
                                   text='مكالمة المصرف'), 'pass')
        self.assertEqual(self.text('حدّد اجتماعًا باسم مراجعة المشروع', {'title': 'مراجعة المشروع'},
                                   title='اجتماع'), 'pass')

    def test_wrong_script_fails(self):
        self.assertEqual(self.text('ذكّرني بالاتصال بالبنك غدًا', {'text': 'الاتصال بالبنك'},
                                   text='call the bank'), 'fail')
        self.assertEqual(self.text('Remind me to call the bank', {'text': 'call the bank'},
                                   text='الاتصال بالبنك'), 'fail')
        self.assertEqual(self.text('Schedule Project Review', {'title': 'Project Review'},
                                   title='مراجعة'), 'fail')

    def test_empty_or_letterless_fails(self):
        for value in ['', '   ', '2026', '١٢٣', None, 5]:
            self.assertEqual(self.text('Remind me to call', {'text': 'call'}, text=value), 'fail', value)
        self.assertEqual(self.text('Remind me to call', {'text': 'call'}), 'fail')

    def test_mixed_prompt_accepts_either_script(self):
        prompt = 'ممكن تعمل schedule لـ meeting مع الـ team يوم الخميس'
        self.assertEqual(self.text(prompt, {'title': 'meeting'}, title='meeting'), 'pass')
        self.assertEqual(self.text(prompt, {'title': 'meeting'}, title='اجتماع الفريق'), 'pass')

    def test_prompt_without_letters_only_needs_text(self):
        self.assertEqual(self.text('11', {'text': 'x'}, text='anything'), 'pass')

    def test_row_reports_arabic_prompt_with_english_reminder(self):
        prompt = {'id': 'command-008', 'category': 'command', 'prompt': 'ذكّرني بالاتصال بالبنك غدًا الساعة العاشرة صباحًا.',
                  'tool': {'name': 'create_reminder', 'args': {'text': 'الاتصال بالبنك', 'time': 'tomorrow 10:00'}}}
        res = {'id': 'command-008', 'tool_calls': [call('create_reminder', text='call the bank',
                                                         time='2026-10-31T10:00:00')]}
        g = self.grade_row(prompt, res)
        self.assertEqual((g['tool'], g['text'], g['exact'], g['time']), ('pass', 'fail', 'fail', 'pass'))
        self.assertTrue(g['issues'][0].startswith('text: text='))

    def grade_row(self, prompt, res):
        return grade.grade_row(prompt, res, TODAY)


class IdentifierArgTests(unittest.TestCase):
    def args(self, expected, **arguments):
        return grade.check_args(expected, arguments)[0]

    def test_path_matches_on_last_folder(self):
        for got in ['Documents', '~/Documents', '/home/sara/documents/', 'C:\\Users\\Sara\\Documents', ' ~/Documents ']:
            self.assertEqual(self.args({'path': '~/Documents'}, path=got), 'pass', got)
            self.assertEqual(self.args({'path': 'Documents'}, path=got), 'pass', got)
        for got in ['~/Downloads', '~', '', '~/Documents/Old', 5, None]:
            self.assertEqual(self.args({'path': '~/Documents'}, path=got), 'fail', got)

    def test_path_is_still_strict_in_exact(self):
        self.assertEqual(grade.check_exact({'path': '~/Documents'}, {'path': 'Documents'})[0], 'fail')
        self.assertEqual(grade.check_exact({'path': '~/Documents'}, {'path': '~/documents'})[0], 'pass')

    def test_name_ignores_case_but_not_language(self):
        self.assertEqual(self.args({'name': 'calculator'}, name=' Calculator '), 'pass')
        self.assertEqual(self.args({'name': 'calculator'}, name='الآلة الحاسبة'), 'fail')

    def test_query_and_lang_keep_their_rules(self):
        self.assertEqual(self.args({'query': 'budget 2026'}, query='Budget 2026'), 'pass')
        self.assertEqual(self.args({'query': 'budget 2026'}, query='budget'), 'fail')
        self.assertEqual(self.args({'lang': 'ar'}, lang='AR'), 'pass')
        self.assertEqual(self.args({'lang': 'ar'}, lang='en'), 'fail')

    def test_free_text_and_time_are_not_in_args(self):
        self.assertEqual(self.args({'text': 'a', 'time': 'tomorrow 10:00'}, x=1), 'pass')


# What `scarfeeos-core chat` prints, written the way core/src/main.rs formats it.
OUTCOMES = {
    'command-001': 'run: OpenApp { name: "Calculator" }',
    'command-002': 'run: OpenApp { name: "calculator" }',
    'command-003': 'run: ListFiles { path: "Documents" }',
    'command-004': 'run: ListFiles { path: "~/Documents" }',
    'command-005': 'run: SearchFiles { query: "budget 2026" }',
    'command-006': 'run: SearchFiles { query: "ميزانية 2026" }',
    'command-007': 'run: CreateReminder { text: "phone the bank", time: "2026-10-31T10:00:00", repeat: None }',
    'command-008': 'run: CreateReminder { text: "اتصل بالبنك", time: "2026-10-31T10:00:00", repeat: None }',
    'command-009': 'run: ScheduleMeeting { title: "Project Review", time: "2026-11-01T14:00:00", '
                   'attendees: ["Sara", "Omar"], repeat: None }',
    'command-010': 'run: ScheduleMeeting { title: "مراجعة المشروع", time: "2026-11-01T14:00:00", '
                   'attendees: ["سارة", "عمر"], repeat: None }',
    'command-011': 'ask the user to confirm before running: SetLanguage { lang: Arabic }',
    'command-012': 'ask the user to confirm before running: SetLanguage { lang: English }',
    'command-013': 'cannot do this offline: weather',
    'command-014': 'cannot tell the time in طوكيو: only the device\'s own time zone is supported',
    'dialect-003': 'run: CreateReminder { text: "أكلم المهندس", time: "2026-10-31T09:00:00", repeat: None }',
    'dialect-006': 'run: OpenApp { name: "email" }',
}

MIXED_MEETING = {
    'id': 'codeswitch-002', 'category': 'codeswitch', 'lang': 'mixed',
    'prompt': 'ممكن تعمل schedule لـ meeting مع الـ team يوم الخميس الساعة 11؟',
    'tool': {'name': 'schedule_meeting', 'args': {'title': 'meeting', 'time': 'Thursday 11:00', 'attendees': ['team']}},
}


class ParseOutcomeTests(unittest.TestCase):
    def call(self, outcome):
        row = grade.parse_outcome(outcome)
        self.assertIsNone(row['error'], outcome)
        self.assertEqual(len(row['tool_calls']), 1, outcome)
        return row, row['tool_calls'][0]['name'], row['tool_calls'][0]['arguments']

    def test_run_formats(self):
        row, name, args = self.call(OUTCOMES['command-007'])
        self.assertEqual((name, row['held']), ('create_reminder', None))
        self.assertEqual(args, {'text': 'phone the bank', 'time': '2026-10-31T10:00:00', 'repeat': None})
        self.assertEqual(self.call(OUTCOMES['command-001'])[1:], ('open_app', {'name': 'Calculator'}))
        self.assertEqual(self.call(OUTCOMES['command-003'])[1:], ('list_files', {'path': 'Documents'}))
        self.assertEqual(self.call(OUTCOMES['command-005'])[1:], ('search_files', {'query': 'budget 2026'}))
        _, name, args = self.call(OUTCOMES['command-010'])
        self.assertEqual((name, args['title'], args['attendees']), ('schedule_meeting', 'مراجعة المشروع', ['سارة', 'عمر']))

    def test_repeat_and_current_time(self):
        _, _, args = self.call('run: CreateReminder { text: "backup", time: "2026-11-06", repeat: Some(Weekly) }')
        self.assertEqual((args['time'], args['repeat']), ('2026-11-06', 'Weekly'))
        _, name, args = self.call('run: CurrentTime { timezone: Some("Tokyo") }')
        self.assertEqual((name, args), ('current_time', {'timezone': 'Tokyo'}))

    def test_held_formats(self):
        row, name, args = self.call(OUTCOMES['command-011'])
        self.assertEqual((row['held'], name, args), ('confirm', 'set_language', {'lang': 'ar'}))
        self.assertEqual(self.call(OUTCOMES['command-012'])[2], {'lang': 'en'})
        row, name, args = self.call('ask the user to confirm the time: ScheduleMeeting { title: "meeting", '
                                    'time: "2026-11-05T11:00:00", attendees: ["team"], repeat: None }')
        self.assertEqual((row['held'], name, args['time']), ('clarify', 'schedule_meeting', '2026-11-05T11:00:00'))
        row, name, args = self.call('ask the user for a date and time: CreateReminder { text: "call the bank", '
                                    'time: "", repeat: None }')
        self.assertEqual((row['held'], name, args['time']), ('missing', 'create_reminder', ''))

    def test_rust_string_escapes(self):
        _, _, args = self.call(r'run: CreateReminder { text: "ذك\u{651}رني \"a\" b\\c\nd\u{64b}", time: "", repeat: None }')
        self.assertEqual(args['text'], 'ذكّرني "a" b\\c\ndً')

    def test_text_outcomes_have_no_call(self):
        for outcome in ['reply: Hello!', 'reply: مرحبا\nwarning: words in the wrong script: Cyrillic "привет"',
                        'time: Friday, 2026-10-30 09:15:00 (device time)', OUTCOMES['command-013'],
                        'cannot tell the time in Tokyo: only the device\'s own time zone is supported']:
            row = grade.parse_outcome(outcome)
            self.assertEqual((row['tool_calls'], row['held'], row['error']), ([], None, None), outcome)
        self.assertIn('Cyrillic', grade.parse_outcome('reply: x\nwarning: Cyrillic "привет"')['reply'])

    def test_errors(self):
        self.assertEqual(grade.parse_outcome('error: model proposed 2 tool calls, expected one')['error'],
                         'error: model proposed 2 tool calls, expected one')
        self.assertEqual(grade.parse_outcome('')['error'], 'no output')
        self.assertEqual(grade.parse_outcome(None)['error'], 'no output')
        for bad in ['run: OpenApp { name: "x }', 'run: OpenApp { name "x" }', 'run: OpenApp { name: "x" } trailing',
                    r'run: OpenApp { name: "\q" }']:
            self.assertIn('unreadable outcome', grade.parse_outcome(bad)['error'], bad)


class PipelineGradeTests(unittest.TestCase):
    def prompts(self):
        return grade.read_jsonl(PROMPTS)

    def graded(self, outcomes, prompts=None):
        rows = [{'id': i, 'outcome': o, 'seconds': 1.0} for i, o in outcomes.items()]
        return grade.grade_all(prompts or self.prompts(), grade.pipeline_results(rows), TODAY)

    def test_every_format_against_prompts_jsonl(self):
        grades = {g['id']: g for g in self.graded(OUTCOMES)}
        for ident in OUTCOMES:
            if ident in ('command-013', 'command-014'):
                continue
            self.assertEqual(grades[ident]['tool'], 'pass', ident)
        self.assertEqual({i for i, g in grades.items() if g['held']}, {'command-011', 'command-012'})
        self.assertEqual({i for i, g in grades.items() if g['exact'] == 'fail'},
                         {'command-003', 'command-007', 'command-008'})
        for ident in ['command-007', 'command-008', 'command-009', 'command-010', 'dialect-003']:
            self.assertEqual((grades[ident]['text'], grades[ident]['time']), ('pass', 'pass'), ident)
        for ident in ['command-001', 'command-003', 'command-005', 'command-009', 'command-011', 'dialect-006']:
            self.assertEqual(grades[ident]['args'], 'pass', ident)
        self.assertEqual([g['issues'] for g in grades.values() if g['category'] == 'command' and g['tool']],
                         [[]] * 12)
        self.assertEqual(grades['command-013']['tool'], None)
        self.assertEqual(grades['qa-001']['issues'], ['no result row'])

    def test_wrong_outcomes_are_failures(self):
        outcomes = {
            'command-001': 'run: SearchFiles { query: "calculator" }',
            'command-008': 'run: CreateReminder { text: "call the bank", time: "2026-10-31T10:00:00", repeat: None }',
            'command-010': 'ask the user for a date and time: ScheduleMeeting { title: "مراجعة", time: "", '
                           'attendees: ["سارة", "عمر"], repeat: None }',
            'command-012': 'reply: Sure, I switched it.',
            'command-002': 'error: could not reach the model',
        }
        grades = {g['id']: g for g in self.graded(outcomes)}
        self.assertEqual((grades['command-001']['tool'], grades['command-001']['held']), ('fail', False))
        self.assertEqual((grades['command-008']['tool'], grades['command-008']['text'], grades['command-008']['time']),
                         ('pass', 'fail', 'pass'))
        self.assertIn('the prompt is Arabic', grades['command-008']['issues'][0])
        g = grades['command-010']
        self.assertEqual((g['tool'], g['held'], g['args'], g['text'], g['time']), ('pass', True, 'pass', 'pass', 'fail'))
        self.assertEqual(grades['command-012']['tool'], 'fail')
        self.assertEqual(grades['command-002']['tool'], 'error')

    def test_bare_hour_clarification_is_a_time_pass_when_the_candidate_is_right(self):
        outcome = ('ask the user to confirm the time: ScheduleMeeting { title: "meeting", '
                   'time: "2026-11-05T11:00:00", attendees: ["team"], repeat: None }')
        g = self.graded({'codeswitch-002': outcome}, [MIXED_MEETING])[0]
        self.assertEqual((g['tool'], g['held'], g['args'], g['text'], g['time'], g['issues']),
                         ('pass', True, 'pass', 'pass', 'pass', []))
        wrong = outcome.replace('11-05', '11-06')
        g = self.graded({'codeswitch-002': wrong}, [MIXED_MEETING])[0]
        self.assertEqual((g['tool'], g['held'], g['time']), ('pass', True, 'fail'))

    def test_leakage_in_a_call_is_found(self):
        outcome = 'run: OpenApp { name: "calculator" }\nwarning: words in the wrong script: Cyrillic "привет"'
        prompt = {'id': 'command-001', 'category': 'command', 'prompt': 'Open the calculator.',
                  'tool': {'name': 'open_app', 'args': {'name': 'calculator'}}}
        self.assertEqual(self.graded({'command-001': outcome}, [prompt])[0]['leakage'], ['Cyrillic'])

    def test_report_has_held_exact_and_text_columns(self):
        text = grade.format_report(self.graded(OUTCOMES), TODAY)
        header = next(l for l in text.splitlines() if l.startswith('category'))
        self.assertEqual(header.split(), ['category', 'prompts', 'tool', 'held', 'args', 'exact', 'text', 'time',
                                          'unchecked', 'leakage'])
        row = next(l for l in text.splitlines() if l.startswith('command'))
        self.assertEqual(row.split(), ['command', '14', '12/12', '2', '10/10', '9/12', '4/4', '4/4', '0', '0'])
        dialect = next(l for l in text.splitlines() if l.startswith('dialect'))
        self.assertEqual(dialect.split()[:6], ['dialect', '7', '2/2', '0', '1/1', '2/2'])


FAKE_EXE = """#!{python}
import sys
OUTCOMES = {outcomes}
assert sys.argv[1] == 'chat' and len(sys.argv) == 3, sys.argv
prompt = sys.argv[2]
import os
out = OUTCOMES.get(prompt, 'reply: lang=' + os.environ.get('SCARFEEOS_LANG', ''))
sys.stdout.buffer.write((out + '\\n').encode('utf-8'))
sys.exit(1 if out.startswith('error:') else 0)
"""


def write_fake_exe(directory, outcomes):
    path = os.path.join(directory, 'fake-core')
    with open(path, 'w', encoding='utf-8') as handle:
        handle.write(FAKE_EXE.format(python=sys.executable, outcomes=repr(outcomes)))
    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR)
    return path


class RunPipelineTests(unittest.TestCase):
    def prompt_outcomes(self):
        by_id = {p['id']: p['prompt'] for p in grade.read_jsonl(PROMPTS)}
        return {by_id[i]: o for i, o in OUTCOMES.items()}

    def test_run_all_writes_id_outcome_seconds(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = os.path.join(tmp, 'fake.py')
            with open(fake, 'w', encoding='utf-8') as handle:
                handle.write(FAKE_EXE.format(python=sys.executable, outcomes=repr(self.prompt_outcomes())))
            out = os.path.join(tmp, 'r.jsonl')
            prompts = [p for p in grade.read_jsonl(PROMPTS) if p['id'] in ('command-001', 'command-008', 'qa-002')]
            lines = []
            count = run_pipeline.run_all([sys.executable, fake], prompts, out, log=lines.append)
            rows = grade.read_jsonl(out)
        self.assertEqual(count, 3)
        self.assertEqual([r['id'] for r in rows], ['command-001', 'command-008', 'qa-002'])
        self.assertEqual(set(rows[0]), {'id', 'outcome', 'seconds'})
        self.assertEqual(rows[0]['outcome'], OUTCOMES['command-001'])
        self.assertEqual(rows[1]['outcome'], OUTCOMES['command-008'])
        self.assertIsInstance(rows[0]['seconds'], float)
        self.assertEqual(len(lines), 3)

    def test_language_follows_the_prompt(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = os.path.join(tmp, 'fake.py')
            with open(fake, 'w', encoding='utf-8') as handle:
                handle.write(FAKE_EXE.format(python=sys.executable, outcomes='{}'))
            for lang, expected in [('en', 'en'), ('ar', 'ar'), ('mixed', 'ar')]:
                outcome, _ = run_pipeline.run_prompt([sys.executable, fake], {'prompt': 'p', 'lang': lang}, 30)
                self.assertEqual(outcome, f'reply: lang={expected}')

    def test_failures_are_recorded_not_raised(self):
        with tempfile.TemporaryDirectory() as tmp:
            row = {'id': 'a', 'prompt': 'p', 'lang': 'en'}
            outcome, _ = run_pipeline.run_prompt([os.path.join(tmp, 'missing')], row, 30)
            self.assertTrue(outcome.startswith('error: cannot run'), outcome)
            silent = os.path.join(tmp, 'silent.py')
            with open(silent, 'w', encoding='utf-8') as handle:
                handle.write('import sys\nsys.stderr.write("boom")\nsys.exit(3)\n')
            outcome, _ = run_pipeline.run_prompt([sys.executable, silent], row, 30)
            self.assertEqual(outcome, 'error: exit code 3, no output: boom')
            slow = os.path.join(tmp, 'slow.py')
            with open(slow, 'w', encoding='utf-8') as handle:
                handle.write('import time\ntime.sleep(30)\n')
            outcome, _ = run_pipeline.run_prompt([sys.executable, slow], row, 1)
            self.assertEqual(outcome, 'error: timed out after 1 seconds')

    @unittest.skipIf(os.name == 'nt', 'the fake executable is a script with a shebang line')
    def test_run_then_grade_with_the_exe_option(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = write_fake_exe(tmp, self.prompt_outcomes())
            out = os.path.join(tmp, 'sub', 'pipeline.jsonl')
            printed = StringIO()
            with redirect_stdout(printed):
                self.assertEqual(run_pipeline.main(['--exe', fake, '--out', out]), 0)
            self.assertEqual(len(grade.read_jsonl(out)), len(grade.read_jsonl(PROMPTS)))
            report = StringIO()
            with redirect_stdout(report):
                code = grade.main([out, '--pipeline', '--today', '2026-10-30'])
        self.assertEqual(code, 0)
        row = next(l for l in report.getvalue().splitlines() if l.startswith('command'))
        self.assertEqual(row.split(), ['command', '14', '12/12', '2', '10/10', '9/12', '4/4', '4/4', '0', '0'])
        self.assertIn('Failures (0):', report.getvalue())


class ReportTests(unittest.TestCase):
    def test_report_totals_and_failures(self):
        prompts = [reminder_prompt(),
                   dict(reminder_prompt(), id='command-008'),
                   {'id': 'qa-001', 'category': 'qa', 'prompt': 'x'}]
        results = [
            dict(result([call('create_reminder', text='call the bank', time='2026-10-31T10:00:00')]),
                 id='command-007'),
            dict(result([call('create_reminder', text='call the bank', time='2026-10-30T10:00:00')],
                        reply='Хорошо'), id='command-008'),
            {'id': 'qa-001', 'reply': 'Fine.'},
        ]
        text = grade.format_report(grade.grade_all(prompts, results, TODAY), TODAY, ['zzz-1'])
        lines = text.splitlines()
        row = next(l for l in lines if l.startswith('command'))
        self.assertEqual(row.split(), ['command', '2', '2/2', '0', '-', '2/2', '2/2', '1/2', '0', '1'])
        self.assertEqual(next(l for l in lines if l.startswith('qa')).split(),
                         ['qa', '1', '-', '0', '-', '-', '-', '-', '0', '0'])
        self.assertIn('Failures (1):', text)
        self.assertIn('command-008  time:', text)
        self.assertIn('command-008  script leakage: Cyrillic', text)
        self.assertIn('zzz-1', text)

    def test_unchecked_is_listed_not_failed(self):
        prompts = [reminder_prompt('next week')]
        results = [result([call('create_reminder', text='call the bank', time='2026-11-06T10:00:00')])]
        text = grade.format_report(grade.grade_all(prompts, results, TODAY), TODAY)
        self.assertIn('Failures (0):', text)
        self.assertIn('Unchecked:', text)
        self.assertIn('command-007  unsupported time description', text)


class MainTests(unittest.TestCase):
    def test_end_to_end_with_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            prompts_path = os.path.join(tmp, 'prompts.jsonl')
            results_path = os.path.join(tmp, 'r.jsonl')
            with open(prompts_path, 'w', encoding='utf-8') as f:
                f.write(json.dumps(reminder_prompt(), ensure_ascii=False) + '\n\n')
            with open(results_path, 'w', encoding='utf-8') as f:
                row = result([call('create_reminder', text='call the bank', time='2026-10-31T10:00:00')])
                f.write(json.dumps(row, ensure_ascii=False) + '\n')
            out = StringIO()
            with redirect_stdout(out):
                code = grade.main([results_path, '--prompts', prompts_path, '--today', '2026-10-30'])
        self.assertEqual(code, 0)
        self.assertIn('Failures (0):', out.getvalue())

    def test_bad_today_is_rejected(self):
        with self.assertRaises(SystemExit):
            grade.main(['x.jsonl', '--today', '30/10/2026'])


DIALECTS = Path(__file__).resolve().parent / 'prompts-dialects.jsonl'
REAL_TOOLS = {'open_app', 'list_files', 'search_files', 'create_reminder', 'schedule_meeting', 'set_language'}
RELIGIOUS_WORDS = {'الله', 'لله', 'بالله', 'شاء', 'ماشاء', 'صلاة', 'الصلاة', 'صلوات', 'قرآن', 'القرآن', 'دعاء',
                   'رمضان', 'مسجد', 'جامع', 'حج', 'عمرة', 'صيام', 'دين', 'الدين', 'ديني', 'god', 'pray', 'prayer',
                   'quran', 'ramadan', 'mosque', 'religion', 'islamic'}


def rust_outcome(prompt, today=TODAY):
    """What `scarfeeos-core chat` would print for a perfect answer to the prompt."""
    tool = prompt.get('tool')
    if not tool:
        return 'reply: a short answer'
    args = dict(tool['args'])
    if 'time' in args:
        when, (start, _) = grade.resolve_description(args['time'], today)
        args['time'] = f'{when.isoformat()}T{start // 60:02d}:{start % 60:02d}:00'

    def quote(value):
        if isinstance(value, list):
            return '[' + ', '.join(quote(v) for v in value) + ']'
        return {'en': 'English', 'ar': 'Arabic'}[value] if tool['name'] == 'set_language' else json.dumps(value, ensure_ascii=False)

    name = ''.join(part.title() for part in tool['name'].split('_'))
    fields = ', '.join(f'{k}: {quote(v)}' for k, v in args.items())
    if tool['name'] in ('create_reminder', 'schedule_meeting'):
        fields += ', repeat: None'
    return f'run: {name} {{ {fields} }}'


class DialectPromptsTests(unittest.TestCase):
    def rows(self):
        return grade.read_jsonl(DIALECTS)

    def test_thirty_prompts_fifteen_per_dialect(self):
        rows = self.rows()
        self.assertEqual(len(rows), 30)
        self.assertEqual([r['dialect'] for r in rows].count('egyptian'), 15)
        self.assertEqual([r['dialect'] for r in rows].count('gulf'), 15)
        ids = [r['id'] for r in rows]
        self.assertEqual(len(set(ids)), 30)
        self.assertFalse(set(ids) & {r['id'] for r in grade.read_jsonl(PROMPTS)})

    def test_fields_and_tools_are_valid(self):
        for row in self.rows():
            self.assertTrue({'id', 'category', 'lang', 'dialect', 'prompt', 'expect'} <= set(row), row['id'])
            self.assertEqual(row['category'], 'dialect', row['id'])
            self.assertIn(row['lang'], ('ar', 'mixed'), row['id'])
            self.assertTrue(row['prompt'].strip() and row['expect'].strip(), row['id'])
            tool = row.get('tool')
            if tool:
                self.assertIn(tool['name'], REAL_TOOLS, row['id'])
                if 'time' in tool['args']:
                    self.assertIsNotNone(grade.resolve_description(tool['args']['time'], TODAY), row['id'])
        self.assertEqual(sum(1 for r in self.rows() if r.get('tool')), 26)

    def test_names_and_politeness_are_present(self):
        text = ' '.join(r['prompt'] for r in self.rows())
        for name in ('إسلام', 'محمد صلاح'):
            self.assertEqual(sum(1 for r in self.rows() if name in r['prompt']), 2, name)
        attendees = [a for r in self.rows() if r.get('tool', {}).get('name') == 'schedule_meeting'
                     for a in r['tool']['args']['attendees']]
        self.assertEqual(sorted(attendees), ['إسلام', 'إسلام', 'محمد صلاح', 'محمد صلاح'])
        self.assertIn('لو سمحت', text)
        self.assertTrue(any(r['lang'] == 'mixed' for r in self.rows()))

    def test_no_religious_content(self):
        for row in self.rows():
            words = {w.casefold() for w in re.findall(r'\w+', row['prompt'] + ' ' + row['expect'])}
            self.assertFalse(words & RELIGIOUS_WORDS, row['id'])

    def test_a_perfect_run_passes_every_check(self):
        rows = self.rows()
        results = grade.pipeline_results([{'id': r['id'], 'outcome': rust_outcome(r), 'seconds': 1} for r in rows])
        grades = grade.grade_all(rows, results, TODAY)
        self.assertEqual([g['issues'] for g in grades], [[]] * 30)
        for g in grades:
            if g['tool'] is not None:
                self.assertEqual((g['tool'], g['exact']), ('pass', 'pass'), g['id'])
        self.assertEqual(sum(1 for g in grades if g['time'] == 'pass'), 12)

    def test_a_translated_or_changed_name_fails(self):
        rows = [r for r in self.rows() if r['id'] in ('dialect-eg-011', 'dialect-gulf-012')]
        outcomes = {'dialect-eg-011': rust_outcome(rows[0]).replace('إسلام', 'Islam'),
                    'dialect-gulf-012': rust_outcome(rows[1]).replace('محمد صلاح', 'محمد')}
        results = grade.pipeline_results([{'id': i, 'outcome': o, 'seconds': 1} for i, o in outcomes.items()])
        grades = grade.grade_all(rows, results, TODAY)
        self.assertEqual([g['args'] for g in grades], ['fail', 'fail'])

    @unittest.skipIf(os.name == 'nt', 'the fake executable is a script with a shebang line')
    def test_run_pipeline_then_grade_with_the_prompts_option(self):
        rows = self.rows()
        with tempfile.TemporaryDirectory() as tmp:
            fake = write_fake_exe(tmp, {r['prompt']: rust_outcome(r) for r in rows})
            out = os.path.join(tmp, 'dialects.jsonl')
            with redirect_stdout(StringIO()) as printed:
                self.assertEqual(run_pipeline.main(['--exe', fake, '--prompts', str(DIALECTS), '--out', out]), 0)
            self.assertIn('Done: 30 prompts', printed.getvalue())
            self.assertEqual([r['id'] for r in grade.read_jsonl(out)], [r['id'] for r in rows])
            report = StringIO()
            with redirect_stdout(report):
                code = grade.main([out, '--pipeline', '--prompts', str(DIALECTS), '--today', '2026-10-30'])
        self.assertEqual(code, 0)
        line = next(l for l in report.getvalue().splitlines() if l.startswith('dialect'))
        self.assertEqual(line.split(), ['dialect', '30', '26/26', '0', '18/18', '26/26', '12/12', '12/12', '0', '0'])
        self.assertIn('Failures (0):', report.getvalue())


if __name__ == '__main__':
    unittest.main()
