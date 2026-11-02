#!/usr/bin/env python3
"""Compare the existing Windows measurement schema using relative limits only."""
import argparse
from decimal import Decimal, InvalidOperation
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
START, END = '<!-- ydeevne:start -->', '<!-- ydeevne:slut -->'
STATIC_KEYS = {
    'koldstart_ms', 'varmstart_ms', 'tomgang_cpu_pct', 'efter_cpu_pct',
    'efter_ws_total_mb', 'efter_pb_total_mb',
    *(f'tomgang_{kind}_{group}_mb' for kind in ('ws', 'pb')
      for group in ('total', 'app', 'webview', 'andre')),
    *(f'diktering_{source}_{stat}_ms' for source in ('log', 'hist')
      for stat in ('p50', 'p95', 'p50_15s', 'p95_15s')
      if source == 'log' or stat in ('p50', 'p95')),
}
PROVIDER_KEY = re.compile(
    r'diktering_(syv|local)_(p50|p95|p50_15s|(?:start|capture|transcribe|polish|insert)_p50)_ms\Z')
CI_KEYS = {'diktering_local_transcribe_p50_ms', 'diktering_local_polish_p50_ms'}


class InputError(Exception):
    pass


class NoBaseline(InputError):
    pass


def load_document(path):
    try:
        text = path.read_text(encoding='utf-8-sig')
        blocks = re.findall(r'^```json\s*\n(.*?)^```\s*$', text, re.M | re.S)
        if len(blocks) > 1:
            raise InputError(f'{path}: forventede kun én JSON-blok.')
        data = json.loads(blocks[0] if blocks else text, parse_float=Decimal)
    except (OSError, UnicodeError, ValueError) as exc:
        raise InputError(f'{path}: kunne ikke læse gyldig JSON ({type(exc).__name__}).') from exc
    if not isinstance(data, dict) or not isinstance(data.get('resultater'), dict):
        raise InputError(f'{path}: JSON skal have et objekt under resultater.')
    return data


def values(data, path):
    result = {}
    for key, value in data['resultater'].items():
        if key not in STATIC_KEYS and not PROVIDER_KEY.fullmatch(key):
            print(f'Advarsel: ukendt måling ignoreres: {key}', file=sys.stderr)
            continue
        if value is None:
            result[key] = None
            continue
        if isinstance(value, bool) or not isinstance(value, (int, float, Decimal)):
            raise InputError(f'{path}: {key} skal være et endeligt, ikke-negativt tal eller null.')
        try:
            number = Decimal(str(value))
        except InvalidOperation as exc:
            raise InputError(f'{path}: ugyldigt tal for {key}.') from exc
        if not number.is_finite() or number < 0:
            raise InputError(f'{path}: {key} skal være et endeligt, ikke-negativt tal.')
        result[key] = number
    return result


def baseline_values(path):
    if not path.exists():
        raise NoBaseline(f'Baseline mangler: {path}. Kør referencemålingen først.')
    data = load_document(path)
    result = values(data, path)
    if data.get('status') == 'ikke_maalt' or not any(v is not None for v in result.values()):
        raise NoBaseline('Baseline er ikke målt endnu. Kør referencemålingen først.')
    return result


def compare(measured, baseline, profile):
    limit = Decimal(40 if profile == 'ci' else 20)
    keys = sorted((measured.keys() | baseline.keys()) & CI_KEYS if profile == 'ci'
                  else measured.keys() | baseline.keys())
    print(f'Profil: {profile}. Tilladt forværring: {limit} %. Lavere er bedre.\n')
    print('| Måling | Baseline | Ændring i % | Resultat |')
    print('|---|---:|---:|---|')
    common, failed = 0, False
    for key in keys:
        old, new = baseline.get(key), measured.get(key)
        if old is None or new is None:
            print(f'| {key} | {old if old is not None else "ikke målt"} | — | ikke målt |')
            continue
        common += 1
        bad = new * 100 > old * (100 + limit)
        change = f'{(new - old) * 100 / old:+.2f}' if old else ('0.00' if new == 0 else '+∞')
        print(f'| {key} | {old} | {change} | {"FEJL" if bad else "OK"} |')
        failed |= bad
    if not common:
        raise NoBaseline('Ingen fælles målte nøgler i den valgte profil.')
    return int(failed)


def render_readme(baseline_path, budgets_path):
    try:
        baseline = baseline_values(baseline_path)
        status = 'Aktuelle baseline-tal:'
    except NoBaseline:
        baseline = {}
        status = 'Baseline: **ikke målt endnu**.'
    try:
        text = budgets_path.read_text(encoding='utf-8')
        candidates = [json.loads(block) for block in re.findall(r'```json\s*\n(.*?)```', text, re.S)]
        budget = next(data for data in candidates if 'budgetter' in data)
    except (OSError, ValueError, StopIteration) as exc:
        raise InputError('Kunne ikke læse budgetterne fra docs/ydeevne.md.') from exc
    rows = ['### Ydeevne', '', status, '', '| Måling | Budget (foreslået) | Baseline |',
            '|---|---:|---:|']
    budgets = {**budget['budgetter'], **budget['ikke_maalt']}
    for key in sorted(budgets.keys() | baseline.keys()):
        unit = 'ms' if key.endswith('_ms') else ('MB' if key.endswith('_mb') else '%')
        proposed = f'{budgets[key]} {unit}' if key in budgets else '—'
        actual = f'{baseline[key]} {unit}' if baseline.get(key) is not None else 'ikke målt'
        rows.append(f'| {key} | {proposed} | {actual} |')
    rows += ['', 'Release-port: højst 20 % forværring i forhold til baseline. '
             'CI-profil: højst 40 % for udvalgte lokale trin; kun relative grænser.', '',
             'Se [målemetoden](docs/ydeevne.md) og [release-checklisten](docs/release-checkliste.md).']
    return '\n'.join(rows)


def main():
    # Redirected output on Windows must retain Danish text and mathematical symbols.
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('maaling', nargs='?', type=Path)
    parser.add_argument('--baseline', type=Path, default=ROOT / 'docs/ydeevne-baseline.json')
    parser.add_argument('--profil', choices=('release', 'ci'), default='release')
    parser.add_argument('--tillad-uden-baseline', action='store_true')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--skriv-readme', action='store_true')
    mode.add_argument('--tjek-readme', action='store_true')
    parser.add_argument('--readme', type=Path, default=ROOT / 'README.md')
    args = parser.parse_args()
    try:
        if args.skriv_readme or args.tjek_readme:
            text = args.readme.read_text(encoding='utf-8')
            if text.count(START) != 1 or text.count(END) != 1 or text.index(START) > text.index(END):
                raise InputError('README skal indeholde ét korrekt ordnet sæt ydeevne-markører.')
            begin, rest = text.split(START)
            _, end = rest.split(END)
            expected = (begin + START + '\n' + render_readme(args.baseline, ROOT / 'docs/ydeevne.md')
                        + '\n' + END + end)
            if args.skriv_readme:
                with args.readme.open('w', encoding='utf-8', newline='\n') as output:
                    output.write(expected)
            elif text != expected:
                print('README er ikke synkroniseret. Kør --skriv-readme.', file=sys.stderr)
                return 1
            return 0
        if args.maaling is None:
            parser.error('angiv en målingsfil eller --tjek-readme/--skriv-readme')
        measured = values(load_document(args.maaling), args.maaling)
        try:
            return compare(measured, baseline_values(args.baseline), args.profil)
        except NoBaseline as exc:
            print(f'Ikke sammenlignet: {exc}', file=sys.stderr)
            return 0 if args.tillad_uden_baseline else 2
    except (InputError, OSError, UnicodeError) as exc:
        print(f'Fejl: {exc}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
