# Eval set

Benchmark prompts used to choose default models. A model that fails the Arabic part is not eligible as a default (see `docs/i18n-and-arabic.md`).

## Files
- `prompts.jsonl`: one JSON object per line.

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

## Review status
Written by Claude, not yet reviewed by a native Arabic speaker. Do not treat scores as final until it is.
