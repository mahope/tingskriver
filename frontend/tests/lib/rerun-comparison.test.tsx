import { afterAll, expect, mock, test } from 'bun:test';
import React, { type ReactNode } from 'react';
import { act, create } from 'react-test-renderer';

const originalEvents = { ...await import('@tauri-apps/api/event') };
const originalWindow = { ...await import('@tauri-apps/api/window') };
const originalDialog = { ...await import('../../src/components/ui/dialog') };
afterAll(() => {
  mock.module('@tauri-apps/api/event', () => originalEvents);
  mock.module('@tauri-apps/api/window', () => originalWindow);
  mock.module('../../src/components/ui/dialog', () => originalDialog);
});

type Payload = { historyId: number; before: string; after: string; changed: boolean };
let receive: (event: { payload: Payload }) => void;
const stop = mock(() => {});
mock.module('@tauri-apps/api/event', () => ({
  ...originalEvents,
  listen: async (name: string, handler: typeof receive) => {
    expect(name).toBe('dictation-rerun-comparison');
    receive = handler;
    return stop;
  },
}));
const show = mock(async () => {});
const unminimize = mock(async () => {});
const setFocus = mock(async () => {});
mock.module('@tauri-apps/api/window', () => ({
  ...originalWindow, getCurrentWindow: () => ({ show, unminimize, setFocus }),
}));
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: ({ open, onOpenChange, children }: { open: boolean; onOpenChange: (open: boolean) => void; children: ReactNode }) =>
    open ? <div><button onClick={() => onOpenChange(false)}>Luk</button>{children}</div> : null,
  DialogContent: ({ children }: { children: ReactNode }) => <section>{children}</section>,
  DialogTitle: ({ children }: { children: ReactNode }) => <h2>{children}</h2>,
  DialogDescription: ({ children }: { children: ReactNode }) => <p>{children}</p>,
}));
const { RerunComparison } = await import('../../src/components/dictation/RerunComparison');

test('a rerun displays both versions, restores the window and can be dismissed', async () => {
  let view: ReturnType<typeof create>;
  await act(async () => { view = create(<RerunComparison />); });
  expect(view!.toJSON()).toBeNull();
  await act(async () => {
    receive({ payload: { historyId: 1, before: 'Første tekst', after: 'Anden tekst', changed: true } });
  });
  const text = JSON.stringify(view!.toJSON());
  expect(text).toContain('Første tekst');
  expect(text).toContain('Anden tekst');
  expect(text).toContain('Teksten er ændret.');
  expect(show).toHaveBeenCalledTimes(1);
  expect(unminimize).toHaveBeenCalledTimes(1);
  expect(setFocus).toHaveBeenCalledTimes(1);
  await act(async () => {
    receive({ payload: { historyId: 1, before: 'Samme', after: 'Samme', changed: false } });
  });
  expect(JSON.stringify(view!.toJSON())).toContain('Teksten er uændret.');
  await act(async () => { view!.root.findByType('button').props.onClick(); });
  expect(view!.toJSON()).toBeNull();
  await act(async () => { view!.unmount(); });
  expect(stop).toHaveBeenCalledTimes(1);
});
