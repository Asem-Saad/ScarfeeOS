import json
import os
import tempfile
import unittest
from contextlib import redirect_stdout
from datetime import date
from io import StringIO

import grade

TODAY = date(2026, 10, 30)  # a Friday


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
        self.assertEqual((g['tool'], g['args'], g['time'], g['issues']), ('pass', 'pass', 'pass', []))

    def test_extra_args_and_dict_arguments_are_accepted(self):
        res = result([{'name': 'create_reminder',
                       'arguments': {'text': 'call the bank', 'time': '2026-10-31T10:00:00', 'x': 1}}])
        g = self.grade(reminder_prompt(), res)
        self.assertEqual((g['tool'], g['args'], g['time']), ('pass', 'pass', 'pass'))

    def test_checks_are_reported_separately(self):
        res = result([call('create_reminder', text='call the dentist', time='2026-10-31T10:00:00')])
        g = self.grade(reminder_prompt(), res)
        self.assertEqual((g['tool'], g['args'], g['time']), ('pass', 'fail', 'pass'))
        res = result([call('create_reminder', text='call the bank', time='2026-10-31T11:00:00')])
        g = self.grade(reminder_prompt(), res)
        self.assertEqual((g['tool'], g['args'], g['time']), ('pass', 'pass', 'fail'))

    def test_languages_are_not_interchangeable(self):
        prompt = {'id': 'command-002', 'category': 'command', 'prompt': 'x',
                  'tool': {'name': 'open_app', 'args': {'name': 'calculator'}}}
        res = {'id': 'command-002', 'reply': '', 'tool_calls': [call('open_app', name='الآلة الحاسبة')]}
        self.assertEqual(self.grade(prompt, res)['args'], 'fail')
        res['tool_calls'] = [call('open_app', name='Calculator')]
        self.assertEqual(self.grade(prompt, res)['args'], 'pass')

    def test_attendees_ignore_order_and_case(self):
        prompt = {'id': 'command-009', 'category': 'command', 'prompt': 'x',
                  'tool': {'name': 'schedule_meeting', 'args': {
                      'title': 'Project Review', 'time': 'Sunday 14:00', 'attendees': ['Sara', 'Omar']}}}
        args = dict(title='project review', time='2026-11-01T14:00:00', attendees=['omar', 'SARA'])
        g = self.grade(prompt, {'id': 'command-009', 'tool_calls': [call('schedule_meeting', **args)]})
        self.assertEqual((g['tool'], g['args'], g['time']), ('pass', 'pass', 'pass'))
        args['attendees'] = ['Sara']
        g = self.grade(prompt, {'id': 'command-009', 'tool_calls': [call('schedule_meeting', **args)]})
        self.assertEqual(g['args'], 'fail')

    def test_missing_arg_and_bad_json(self):
        g = self.grade(reminder_prompt(), result([call('create_reminder', time='2026-10-31T10:00:00')]))
        self.assertEqual((g['args'], g['time']), ('fail', 'pass'))
        g = self.grade(reminder_prompt(), result([{'name': 'create_reminder', 'arguments': '{oops'}]))
        self.assertEqual((g['tool'], g['args'], g['time']), ('pass', 'fail', 'fail'))

    def test_wrong_or_no_tool(self):
        g = self.grade(reminder_prompt(), result([call('open_app', name='x')]))
        self.assertEqual((g['tool'], g['args'], g['time']), ('fail', 'na', 'na'))
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
        self.assertEqual((g['tool'], g['args'], g['time']), ('pass', 'pass', 'unchecked'))

    def test_prompt_without_tool_only_checks_leakage(self):
        prompt = {'id': 'qa-001', 'category': 'qa', 'prompt': 'x'}
        g = self.grade(prompt, {'id': 'qa-001', 'reply': 'Ответ', 'tool_calls': []})
        self.assertEqual((g['tool'], g['args'], g['time'], g['leakage']), (None, None, None, ['Cyrillic']))
        self.assertEqual(self.grade(prompt, {'id': 'qa-001', 'reply': None})['leakage'], [])

    def test_leakage_is_checked_on_tool_prompts_too(self):
        res = result([call('create_reminder', text='call the bank', time='2026-10-31T10:00:00')], reply='好的')
        g = self.grade(reminder_prompt(), res)
        self.assertEqual((g['tool'], g['leakage']), ('pass', ['CJK']))

    def test_run_error_and_missing_row(self):
        g = self.grade(reminder_prompt(), result(error='timeout'))
        self.assertEqual((g['tool'], g['args'], g['time']), ('error', 'na', 'na'))
        g = self.grade(reminder_prompt(), None)
        self.assertEqual((g['tool'], g['args'], g['time']), ('missing', 'na', 'na'))


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
        self.assertEqual(row.split(), ['command', '2', '2/2', '2/2', '1/2', '0', '1'])
        self.assertEqual(next(l for l in lines if l.startswith('qa')).split(),
                         ['qa', '1', '-', '-', '-', '0', '0'])
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


if __name__ == '__main__':
    unittest.main()
