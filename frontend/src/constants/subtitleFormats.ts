export const SUBTITLE_EXTENSIONS = ['vtt', 'srt'] as const;

export function isSubtitleExtension(ext: string): boolean {
  return (SUBTITLE_EXTENSIONS as readonly string[]).includes(ext.toLowerCase());
}
