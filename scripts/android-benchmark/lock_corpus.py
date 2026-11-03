#!/usr/bin/env python3
"""Lock locally consented PCM WAV files; never copy voices into the repository."""
import argparse
import hashlib
import json
from pathlib import Path
import wave


def lock(audio_dir, consent):
    if not consent.strip():
        raise ValueError('Consent record required')
    data = json.loads(Path(__file__).with_name('corpus.json').read_text())
    data['consent'] = consent
    for clip in data['clips']:
        path = audio_dir / (clip['id'] + '.wav')
        with wave.open(str(path), 'rb') as audio:
            if (audio.getnchannels(), audio.getsampwidth(), audio.getframerate(),
                    audio.getnframes(), audio.getcomptype()) != (1, 2, 16000, 240000, 'NONE'):
                raise ValueError('Expected exactly 15 seconds mono PCM16 at 16 kHz')
        clip['sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
    return data


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('audio_dir', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--consent', required=True, help='Local consent record ID, no names')
    args = parser.parse_args()
    try:
        data = lock(args.audio_dir, args.consent)
        # Exclusive creation: a locked corpus cannot accidentally be overwritten.
        with args.output.open('x') as output:
            json.dump(data, output, ensure_ascii=False, indent=2)
            output.write('\n')
    except (ValueError, OSError, wave.Error):
        parser.exit(2, 'Corpus lock failed; check consent, WAV format and output path\n')
