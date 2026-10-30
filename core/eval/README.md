# Eval set

Benchmark prompts used to choose default models. A model that fails the Arabic part is not eligible as a default (see `docs/i18n-and-arabic.md`).

## Files
- `prompts.jsonl`: one JSON object per line. 51 prompts.
- `run_eval.ps1`: sends each prompt to a local llama-server and writes `results/<label>.jsonl` (model replies only).
- `run_pipeline.py`: sends each prompt through `scarfeeos-core chat` and writes `results/pipeline.jsonl` (the whole assistant, not just the model).
- `grade.py`: grades either results file against `prompts.jsonl`. `test_grade.py` tests it (`python3 -m unittest discover -s .`).

`results/` is not committed.

## Removed prompts
Two religious prompts (`qa-009`, `qa-010`, the five pillars of Islam in English and Arabic) were removed because religious content is forbidden in ScarfeeOS: the assistant declines it. The prompt count is 51.

## Fields
| Field | Meaning |
|-------|---------|
| `id` | Unique id, `<category>-<number>` |
| `category` | `command`, `qa`, `codeswitch`, `dialect`, `writing`, `format`, `regression` |
| `lang` | `en`, `ar`, or `mixed` |
| `dialect` | Only for dialect prompts: `msa`, `gulf`, `egyptian`, `levantine` |
| `prompt` | The user message |
| `expect` | What a correct answer must do (graded by a human or a checker) |
| `tool` | For `command` prompts: the expected tool call, `{name, args}`. `time` here is a description such as `tomorrow 10:00`, to be resolved against the run date; the model itself must return ISO 8601. |

## Tool schema (draft, mirrors the future core tool API)
- `open_app(name)`
- `list_files(path)`
- `search_files(query)`
- `create_reminder(text, time)`, `time` as ISO 8601
- `schedule_meeting(title, time, attendees)`
- `set_language(lang)`, `lang` is `en` or `ar`

## Grading
- `command`: tool name and required args match (argument text may be in the user's language).
- `qa`, `writing`: human grade 0 to 2 for correctness and 0 to 2 for fluency.
- `codeswitch`, `dialect`: the answer must understand the request and reply in the language the user used.
- `format`: numerals, dates and direction handled as asked.
- `regression`: MSA prompts re-run after any fine-tune to detect forgetting.

## Running the whole pipeline
```
python3 run_pipeline.py --exe ../target/release/scarfeeos-core [--out results/pipeline.jsonl] [--prompts prompts.jsonl] [--timeout 300]
python3 grade.py results/pipeline.jsonl --pipeline --today 2026-10-30
```
- Start the local model server first (`http://127.0.0.1:8080`); the executable talks to it itself.
- `run_pipeline.py` runs `scarfeeos-core chat "<prompt>"` once per prompt, with `SCARFEEOS_LANG` set to `en` for `en` prompts and `ar` for `ar` and `mixed` prompts.
- Each output line is `{"id", "outcome", "seconds"}`. `outcome` is what the executable printed, or `error: ...` if it timed out, could not run, or printed nothing.
- `grade.py --pipeline` reads the printed `run: `, `ask the user to confirm the time: `, `ask the user to confirm before running: ` and `ask the user for a date and time: ` lines back into tool calls. Anything else is a text reply.
- `--today` is required: the run date, `YYYY-MM-DD`. Weekdays and "tomorrow" in `tool.args.time` are resolved against it, so use the date the run was made.

## Grader columns
Per category, `passed/graded`, or `-` when nothing was graded. Only prompts with an expected `tool` get these.

| Column | Meaning |
|--------|---------|
| `prompts` | Prompts in the category |
| `tool` | A tool call with the expected name exists. A call the assistant holds back to ask the user counts as a pass |
| `held` | Count of `tool` passes where the assistant asked the user first (confirm the time, confirm before running, ask for a date and time). A count, not a ratio |
| `args` | Identifier args match: `name`, `query`, `lang` and `attendees` equal the expected value (trimmed, case-insensitive, no translation). A `list_files` `path` matches on its last folder name |
| `exact` | Every expected arg except `time`, strictly, with free text included and the path in full. A paraphrase fails on purpose |
| `text` | Free-text args (reminder `text`, meeting `title`) are non-empty and in the script of the prompt, Arabic or Latin. Any wording passes |
| `time` | The `time` arg matches the expected description resolved against `--today`. A time the assistant asked the user to confirm is graded as its candidate. A prompt the assistant said has no date or time fails |
| `unchecked` | Count of `time` args whose description the grader does not understand |
| `leakage` | Count of replies with Cyrillic or CJK characters. Checked on every prompt, not only commands |

- `args`, `exact`, `text` and `time` are not graded when no matching tool call exists, and `text` is graded only when the expected args include free text.
- Understood time descriptions: `tomorrow HH:MM`, `<Weekday> HH:MM`, `tomorrow morning`. A weekday means its next occurrence after `--today`; morning is 05:00 to 11:59. A UTC offset in the model's ISO 8601 time is ignored.
- Failures are listed under the table, one line per issue. Result ids that are not in the prompts file are ignored and named.
- `qa`, `writing`, `codeswitch`, `dialect`, `format` and `regression` prompts have no expected tool, so only `leakage` is checked for them. The rest is human grading.

## Review status
Written by Claude, not yet reviewed by a native Arabic speaker. Do not treat scores as final until it is.
