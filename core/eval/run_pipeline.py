#!/usr/bin/env python3
"""Runs every prompt in prompts.jsonl through `scarfeeos-core chat "<prompt>"`.

    python3 run_pipeline.py --exe ../target/release/scarfeeos-core [--out results/pipeline.jsonl]

The executable talks to the local model server itself (http://127.0.0.1:8080), so start that first.
Writes one JSON line per prompt: {"id", "outcome", "seconds"}, where outcome is what the
executable printed. Grade it with: python3 grade.py <out> --pipeline --today YYYY-MM-DD

SCARFEEOS_LANG is set to the prompt's language (ar for Arabic and mixed prompts), as a user
with that system language would have it.
"""
import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path


def run_prompt(command, row, timeout):
    """Returns (outcome, seconds) for one prompt. Failures to run are written as `error: ...`."""
    env = dict(os.environ, SCARFEEOS_LANG='en' if row.get('lang') == 'en' else 'ar')
    started = time.monotonic()
    try:
        done = subprocess.run(command + ['chat', row['prompt']], capture_output=True, env=env, timeout=timeout)
    except subprocess.TimeoutExpired:
        outcome = f'error: timed out after {timeout} seconds'
    except OSError as exc:
        outcome = f'error: cannot run {command[-1]}: {exc}'
    else:
        outcome = done.stdout.decode('utf-8', errors='replace').strip()
        if not outcome:
            stderr = done.stderr.decode('utf-8', errors='replace').strip()
            outcome = f'error: exit code {done.returncode}, no output' + (f': {stderr}' if stderr else '')
    return outcome, round(time.monotonic() - started, 1)


def run_all(command, prompts, out_path, timeout=300, log=print):
    with open(out_path, 'w', encoding='utf-8') as out:
        for row in prompts:
            outcome, seconds = run_prompt(command, row, timeout)
            out.write(json.dumps({'id': row['id'], 'outcome': outcome, 'seconds': seconds}, ensure_ascii=False) + '\n')
            out.flush()
            log(f"{row['id']}  {seconds}s")
    return len(prompts)


def main(argv=None):
    here = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description='Run prompts.jsonl through scarfeeos-core chat.')
    parser.add_argument('--exe', required=True, help='path to the scarfeeos-core executable')
    parser.add_argument('--prompts', default=str(here / 'prompts.jsonl'))
    parser.add_argument('--out', default=str(here / 'results' / 'pipeline.jsonl'))
    parser.add_argument('--timeout', type=int, default=300, help='seconds allowed per prompt')
    args = parser.parse_args(argv)

    with open(args.prompts, encoding='utf-8-sig') as handle:
        prompts = [json.loads(line) for line in handle if line.strip()]
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    count = run_all([args.exe], prompts, args.out, args.timeout)
    print(f'Done: {count} prompts. Results: {args.out}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
