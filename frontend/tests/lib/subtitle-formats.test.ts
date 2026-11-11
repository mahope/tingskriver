import { describe, expect, test } from 'bun:test';
import { isSubtitleExtension } from '../../src/constants/subtitleFormats';

describe('subtitle formats', () => {
  test('accepts vtt and srt in any case, nothing else', () => {
    expect(isSubtitleExtension('vtt')).toBe(true);
    expect(isSubtitleExtension('SRT')).toBe(true);
    expect(isSubtitleExtension('mp3')).toBe(false);
    expect(isSubtitleExtension('txt')).toBe(false);
  });
});
