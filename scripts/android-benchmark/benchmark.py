#!/usr/bin/env python3
"""Offline Android Whisper scoring. Inputs stay local; output contains only metrics."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import unicodedata

MODELS = ('base', 'small', 'small-q5_1')
TIERS = ('midrange', 'recent')


def tokens(text):
    text = unicodedata.normalize('NFC', text).lower()
    return ''.join(c if c.isalnum() else ' ' for c in text).split()


def distance(a, b):
    previous = list(range(len(b) + 1))
    for i, left in enumerate(a, 1):
        current = [i]
        for j, right in enumerate(b, 1):
            current.append(min(current[-1] + 1, previous[j] + 1,
                               previous[j-1] + (left != right)))
        previous = current
    return previous[-1]


def percentile(values, fraction):
    return sorted(values)[math.ceil(len(values) * fraction) - 1]


def require(condition):
    if not condition:
        raise ValueError('Invalid or incomplete benchmark input; see protocol')


def digest(value, length=64):
    require(isinstance(value, str) and re.fullmatch('[0-9a-f]{%d}' % length, value))


def summarize(data):
    require(bool(data['consent'].strip()))
    digest(data['engine_revision'], 40)
    clips = {c['id']: c for c in data['clips']}
    require(len(clips) == len(data['clips']))
    require({c['kind'] for c in clips.values()} == {'quiet', 'noise', 'silence'})
    for clip in clips.values():
        digest(clip['sha256'])
        require(clip['duration_s'] == 15)
        require(bool(tokens(clip['reference'])) == (clip['kind'] != 'silence'))
    runs = data['runs']
    require({r['tier'] for r in runs} == set(TIERS))
    require({r['model'] for r in runs} == set(MODELS))
    identities = set()
    for row in runs:
        require(row['clip'] in clips and row['abi'] == 'arm64-v8a')
        require(all(isinstance(row[k], str) and row[k].strip()
                    for k in ('hardware', 'os')))
        digest(row['model_sha256'])
        for key in ('cold_ms', 'warm_ms', 'peak_pss_mb', 'model_bytes',
                    'energy_mwh', 'temperature_start_c', 'temperature_end_c',
                    'thermal_status'):
            value = row[key]
            require(type(value) in (int, float) and math.isfinite(value) and value >= 0)
        require(row['model_bytes'] > 0 and row['peak_pss_mb'] > 0)
        require(type(row['repeat']) is int and 0 <= row['repeat'] < 20)
        identity = (row['tier'], row['model'], row['clip'], row['repeat'])
        require(identity not in identities)
        identities.add(identity)
    require(len(identities) == 2 * 3 * len(clips) * 20)
    output = []
    for tier in TIERS:
        for model in MODELS:
            rows = [r for r in runs if r['tier'] == tier and r['model'] == model]
            for key in ('hardware', 'os', 'model_sha256', 'model_bytes'):
                require(len({r[key] for r in rows}) == 1)
            result = dict(tier=tier, model=model, model_bytes=rows[0]['model_bytes'])
            for kind in ('quiet', 'noise'):
                selected = [r for r in rows if clips[r['clip']]['kind'] == kind]
                for metric, chars in (('wer', False), ('cer', True)):
                    errors = total = 0
                    for row in selected:
                        ref = tokens(clips[row['clip']]['reference'])
                        hyp = tokens(row['hypothesis'])
                        if chars:
                            ref, hyp = ''.join(ref), ''.join(hyp)
                        errors += distance(ref, hyp)
                        total += len(ref)
                    result[f'{metric}_{kind}'] = errors / total
            speech = [r for r in rows if clips[r['clip']]['kind'] != 'silence']
            for key in ('cold_ms', 'warm_ms'):
                for name, p in (('p50', .5), ('p95', .95)):
                    result[f'{key}_{name}'] = percentile([r[key] for r in speech], p)
            for key in ('peak_pss_mb', 'thermal_status', 'temperature_end_c'):
                result[f'{key}_max'] = max(r[key] for r in rows)
            result['energy_mwh_total'] = sum(r['energy_mwh'] for r in rows)
            result['silence_outputs'] = sum(bool(r['hypothesis'].strip()) for r in rows
                                            if clips[r['clip']]['kind'] == 'silence')
            result['accepted'] = (result['wer_quiet'] <= .15 and
                                  result['warm_ms_p95'] <= 3000 and
                                  result['silence_outputs'] == 0 and
                                  (model != 'base' or result['peak_pss_mb_max'] <= 700))
            output.append(result)
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input', type=Path)
    parser.add_argument('--audio-dir', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    try:
        data = json.loads(args.input.read_text())
        for clip in data['clips']:
            require(re.fullmatch('[a-z0-9-]+', clip['id']))
            audio = args.audio_dir / (clip['id'] + '.wav')
            require(hashlib.sha256(audio.read_bytes()).hexdigest() == clip['sha256'])
        result = summarize(data)
        args.output.write_text(json.dumps(result, indent=2) + '\n')
    except (ValueError, KeyError, TypeError, OSError):
        parser.exit(2, 'Benchmark rejected: invalid input or missing/hash-mismatched audio\n')
    return 0 if all(r['accepted'] for r in result) else 1


if __name__ == '__main__':
    raise SystemExit(main())
