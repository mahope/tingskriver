import { afterAll, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

const originalEvents = { ...await import('@tauri-apps/api/event') };
const originalService = { ...await import('../../src/services/recordingService') };
const originalToast = { ...await import('sonner') };
afterAll(() => {
  mock.module('@tauri-apps/api/event', () => originalEvents);
  mock.module('../../src/services/recordingService', () => originalService);
  mock.module('sonner', () => originalToast);
});

const listeners = new Map<string, (event: { payload: { message: string } }) => void>();
const removed: string[] = [];
mock.module('@tauri-apps/api/event', () => ({
  listen: async (name: string, callback: (event: { payload: { message: string } }) => void) => {
    listeners.set(name, callback);
    return () => { listeners.delete(name); removed.push(name); };
  },
}));
mock.module('../../src/services/recordingService', () => ({
  recordingService: new Proxy({}, {
    get: (_, name) => name === 'getRecordingState'
      ? async () => ({ is_recording: false, is_paused: false, is_active: false })
      : async () => () => {},
  }),
}));
const warning = mock(() => {});
const success = mock(() => {});
const error = mock(() => {});
mock.module('sonner', () => ({ toast: { warning, success, error } }));
const { RecordingStateProvider } = await import('../../src/contexts/RecordingStateContext');

test('disk transitions show and replace a persistent warning independently of write errors', async () => {
  let renderer: ReturnType<typeof create>;
  await act(async () => { renderer = create(<RecordingStateProvider><span /></RecordingStateProvider>); });
  try {
    expect(listeners.has('recording-disk-low')).toBe(true);
    expect(listeners.has('recording-disk-ok')).toBe(true);
    listeners.get('recording-disk-low')!({ payload: { message: 'Low space' } });
    expect(warning).toHaveBeenCalledWith('Low space', { id: 'recording-disk', duration: Infinity });
    listeners.get('recording-storage-error')!({ payload: { message: 'Write failed' } });
    expect(error).toHaveBeenCalledWith('Write failed', { id: 'recording-storage', duration: Infinity });
    listeners.get('recording-disk-ok')!({ payload: { message: 'Space recovered' } });
    expect(success).toHaveBeenCalledWith('Space recovered', { id: 'recording-disk', duration: 5000 });
  } finally {
    await act(async () => renderer!.unmount());
  }
  expect(removed).toContain('recording-disk-low');
  expect(removed).toContain('recording-disk-ok');
});
