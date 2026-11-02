import { describe, expect, test } from 'bun:test';
import { recordingFinalized, waitForRecordingAudio, transcribeSavedRecording } from '../../src/lib/recording-finalization';

describe('recording finalization', () => {
  test('transcribes only after finalization, using the saved meeting id and folder', async () => {
    const calls: string[][] = [];
    const job = transcribeSavedRecording('saved-folder', 'database-id', async (id, folder) => {
      calls.push([id, folder]);
    });
    await Promise.resolve();
    expect(calls).toEqual([]);
    recordingFinalized({ meeting_folder: 'saved-folder', status: 'ok', message: '', audio_file: 'audio.mp4' });
    await job;
    expect(calls).toEqual([['database-id', 'saved-folder']]);
  });

  test('retains completion before the meeting has been saved', async () => {
    recordingFinalized({ meeting_folder: 'early', status: 'ok', message: '', audio_file: 'audio.mp4' });
    await expect(waitForRecordingAudio('early')).resolves.toBeUndefined();
  });

  test('waits for the matching recording and permits usable partial audio', async () => {
    let ready = false;
    const pending = waitForRecordingAudio('late').then(() => { ready = true; });
    recordingFinalized({ meeting_folder: 'other', status: 'ok', message: '', audio_file: 'audio.mp4' });
    await Promise.resolve();
    expect(ready).toBe(false);
    recordingFinalized({ meeting_folder: 'late', status: 'delvis', message: '', audio_file: 'audio.mp4' });
    await pending;
    expect(ready).toBe(true);
  });

  test('reports failure instead of transcribing a missing file', async () => {
    recordingFinalized({ meeting_folder: 'failed', status: 'fejlet', message: 'Lyd mangler' });
    await expect(waitForRecordingAudio('failed')).rejects.toThrow('Lyd mangler');
  });
});
