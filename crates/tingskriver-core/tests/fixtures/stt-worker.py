#!/usr/bin/python3
import os, signal, struct, sys, time
r, w = sys.stdin.buffer, sys.stdout.buffer
def exact(n):
    result = bytearray()
    while len(result) < n:
        part = r.read(n - len(result))
        if not part: raise EOFError()
        result.extend(part)
    return result
def fault(phase):
    spec = os.environ.get('TINGSKRIVER_STT_FAULT', '')
    gpu_only = spec.endswith(':gpu')
    spec = spec.removesuffix(':gpu')
    if gpu_only and os.environ.get('TINGSKRIVER_STT_DEVICE') == 'cpu': return
    if '@' not in spec: return
    kind, at = spec.split('@')
    if at != phase: return
    if kind == 'hang':
        time.sleep(3600)
    if kind == 'reject': return True
    sys.stderr.write(f'fake worker: {kind} at {phase}\n')
    sys.stderr.flush()
    os.kill(os.getpid(), signal.SIGABRT if kind == 'abort' else signal.SIGSEGV)
fault('init')
count = 0
while True:
    try:
        i, c, n = struct.unpack('<QII', exact(16))
        control, pcm = exact(c), exact(n * 4)
    except EOFError: break
    count += 1
    rejected = fault('load' if count == 1 else 'run')
    if control == b'hang': time.sleep(3600)
    if control == b'crash': os.kill(os.getpid(), signal.SIGKILL)
    if control == b'wrong-id': i += 1
    response = b'fail' if rejected else b'null'
    w.write(struct.pack('<QII', i, 4, n) + response + pcm)
    w.flush()
