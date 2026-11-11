#!/usr/bin/env python3
"""Linux real-time two-track soak. Uses only the Python standard library."""
import argparse
import json
from pathlib import Path
import subprocess
import time


# Finishing a three-hour recording encodes three AAC files and verifies them.
# Measured at about 0.03 s of work per recorded second on a developer machine
# (about 0.07 s before the outputs were encoded side by side), so the limit
# only has to catch a hang, not police speed.
POST_STEP_TIMEOUT = 1200


def check_memory(values):
    assert values and max(values) - values[0] < 100_000_000, 'RAM growth >= 100 MB'


def metrics(pid, folder):
    status = Path(f'/proc/{pid}/status').read_text()
    rss = int(next(line.split()[1] for line in status.splitlines() if line.startswith('VmRSS:'))) * 1024
    # utime + stime, in clock ticks; process name can contain spaces.
    fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
    return dict(rss_bytes=rss, cpu_ticks=int(fields[11]) + int(fields[12]),
                file_bytes=sum(p.stat().st_size for p in folder.rglob('*.wav')))


def run(binary, root, seconds, crash=False, interval=300):
    folder = root / ('crash' if crash else 'normal')
    folder.mkdir(parents=True, exist_ok=False)
    child = subprocess.Popen([str(binary), 'record', str(folder), str(seconds), '--measure-final'])
    values = []
    final_measured = False
    try:
        deadline = time.monotonic() + 60
        while not (folder / 'ready').exists():
            assert child.poll() is None, 'recorder exited before ready'
            assert time.monotonic() < deadline, 'recorder startup timeout'
            time.sleep(0.01)
        start = time.monotonic()
        next_log = 0
        while child.poll() is None:
            elapsed = time.monotonic() - start
            if elapsed >= next_log:
                sample = metrics(child.pid, folder)
                values.append(sample['rss_bytes'])
                print(json.dumps(dict(variant=folder.name, elapsed_s=elapsed, **sample)), flush=True)
                check_memory(values)
                next_log += interval
            if not crash and (folder / 'finished').exists() and not (folder / 'measured').exists():
                sample = metrics(child.pid, folder)
                values.append(sample['rss_bytes'])
                print(json.dumps(dict(variant=folder.name, elapsed_s=elapsed, final=True, **sample)), flush=True)
                check_memory(values)
                (folder / 'measured').touch()
                final_measured = True
            if crash and elapsed >= seconds / 2:
                child.kill()
                child.wait(timeout=10)
                subprocess.run([str(binary), 'recover', str(folder), str(elapsed)], check=True, timeout=POST_STEP_TIMEOUT)
                return
            assert elapsed < seconds + 60, 'recorder timeout'
            time.sleep(0.05)
        assert not crash, 'recorder exited before forced stop'
        assert child.returncode == 0, f'recorder failed: {child.returncode}'
        assert final_measured, 'recorder exited without final memory measurement'
        subprocess.run([str(binary), 'finish', str(folder), str(seconds)], check=True, timeout=POST_STEP_TIMEOUT)
    finally:
        if child.poll() is None:
            child.kill()
        child.wait(timeout=10)


def run_isolation(binary, root, phase_seconds, thresholds):
    """Pilot-tone channel isolation (#189) on the finished tracks of a short extra recording."""
    folder = root / 'isolation'
    folder.mkdir(parents=True, exist_ok=False)
    subprocess.run([str(binary), 'isolation', str(folder), str(phase_seconds), str(thresholds)],
                   check=True, timeout=phase_seconds * 3 + 300)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--seconds', type=float, default=10800)
    parser.add_argument('--interval', type=float, default=300)
    parser.add_argument('--isolation-seconds', type=float, default=3,
                        help='seconds per phase of the pilot-tone isolation check; 0 skips it')
    parser.add_argument('--thresholds', type=Path,
                        default=Path(__file__).resolve().parent.parent / 'docs' / 'kanalisolation-graenser.json')
    args = parser.parse_args()
    if args.seconds < 4 or args.interval <= 0:
        parser.error('seconds must be >= 4; interval must be positive')
    run(args.binary.resolve(), args.output, args.seconds, interval=args.interval)
    run(args.binary.resolve(), args.output, args.seconds, crash=True, interval=args.interval)
    if args.isolation_seconds > 0:
        run_isolation(args.binary.resolve(), args.output, args.isolation_seconds, args.thresholds)
