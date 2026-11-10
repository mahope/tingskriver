export interface FinalizedRecording {
  meeting_folder: string;
  status: string;
  message: string;
  audio_file?: string;
}

const completed = new Map<string, FinalizedRecording>();
const waiting = new Map<string, Array<(result: FinalizedRecording) => void>>();

export function recordingFinalized(result: FinalizedRecording): void {
  completed.set(result.meeting_folder, result);
  if (completed.size > 20) completed.delete(completed.keys().next().value!);
  waiting.get(result.meeting_folder)?.forEach(resolve => resolve(result));
  waiting.delete(result.meeting_folder);
}

export async function waitForRecordingAudio(folder: string): Promise<void> {
  const result = completed.get(folder) ?? await new Promise<FinalizedRecording>(resolve => {
    waiting.set(folder, [...(waiting.get(folder) ?? []), resolve]);
  });
  if (!result.audio_file || result.status === 'fejlet') throw new Error(result.message);
}

export async function transcribeSavedRecording(
  folder: string,
  meetingId: string,
  transcribe: (meetingId: string, folder: string) => Promise<void>,
): Promise<void> {
  await waitForRecordingAudio(folder);
  await transcribe(meetingId, folder);
}
