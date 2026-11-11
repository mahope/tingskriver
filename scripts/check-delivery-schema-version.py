"""Reject delivery contract changes without an increased schema version."""
import argparse
import json
import re
from pathlib import Path
import subprocess
import sys


def git(*args):
    return subprocess.check_output(['git', *args], text=True).strip()


def check(base):
    merge_base = git('merge-base', base, 'HEAD')
    errors = []
    published = set()
    versions = []
    for filename in git('ls-tree', '-r', '--name-only', merge_base,
                        'docs/schemas').splitlines():
        path = Path(filename)
        if not path.match('docs/schemas/delivery-v*.schema.json'):
            continue
        before = json.loads(git('show', f'{merge_base}:{filename}'))
        published.add(path)
        old_version = before.get('properties', {}).get('skemaversion', {}).get('const')
        if type(old_version) is not int:
            raise ValueError('Invalid published schema version')
        versions.append(old_version)
        if not path.exists():
            errors.append(f'{filename}: behold den publicerede kontrakt')
            continue
        after = json.loads(path.read_text(encoding='utf-8'))
        if before == after:
            continue
        new_version = after.get('properties', {}).get('skemaversion', {}).get('const')
        version_bumped = (type(new_version) is int and type(old_version) is int
                          and new_version > old_version)
        if not version_bumped:
            errors.append(f'{filename}: skemaændring kræver højere skemaversion.const')
    for path in Path('docs/schemas').glob('delivery-v*.schema.json'):
        if path in published:
            continue
        match = re.fullmatch(r'delivery-v([0-9]+)\.schema\.json', path.name)
        schema = json.loads(path.read_text(encoding='utf-8'))
        version = schema.get('properties', {}).get('skemaversion', {}).get('const')
        if (not match or type(version) is not int or int(match[1]) != version
                or version <= max(versions, default=0)):
            errors.append(f'{path}: filversion skal svare til const og overstige udgivne versioner')
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', default='origin/main')
    args = parser.parse_args()
    try:
        errors = check(args.base)
    except (subprocess.CalledProcessError, ValueError, OSError) as error:
        print(f'Skemakontrol kunne ikke gennemføres: {type(error).__name__}', file=sys.stderr)
        return 1
    for error in errors:
        print(error, file=sys.stderr)
    return int(bool(errors))


if __name__ == '__main__':
    sys.exit(main())
