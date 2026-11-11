#!/usr/bin/python3
import os, signal, struct, sys
r, w = sys.stdin.buffer, sys.stdout.buffer
def exact(n):
    result = bytearray()
    while len(result) < n:
        part = r.read(n - len(result))
        if not part: raise EOFError()
        result.extend(part)
    return result
while True:
    try:
        i, c, n = struct.unpack('<QII', exact(16))
        control, pcm = exact(c), exact(n * 4)
    except EOFError: break
    if control == b'crash': os.kill(os.getpid(), signal.SIGKILL)
    if control == b'wrong-id': i += 1
    w.write(struct.pack('<QII', i, 4, n) + b'null' + pcm)
    w.flush()
