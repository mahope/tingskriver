import { expect, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { DeliverySettings } from '../../src/components/delivery/DeliverySettings';
import { DeliveryApproval } from '../../src/components/delivery/DeliveryApproval';

test('delivery starts closed, tests use the selected target, and failures stay visible', async () => {
  const calls: string[] = [];
  const request = async <T,>(command: string): Promise<T> => {
    calls.push(command);
    if (command === 'delivery_status') return {
      policy: { local_only: true, private: false },
      targets: [{ id: 'one', name: 'Arkiv', destination: { kind: 'folder', path: '/archive' } }],
      jobs: [{ id: 'job', target_id: 'one', status: 'failed', attempts: 5, next_attempt: 0, error: 'Aflevering mislykkedes' }],
    } as T;
    return undefined as T;
  };
  let renderer: ReturnType<typeof create>;
  await act(async () => { renderer = create(<DeliverySettings request={request} />); });
  const buttons = renderer!.root.findAllByType('button');
  const testButton = buttons.find(b => b.children.join('') === 'Test mål');
  expect(testButton?.props.disabled).toBe(true);
  expect(JSON.stringify(renderer!.toJSON())).toContain('Aflevering mislykkedes');
  expect(JSON.stringify(renderer!.toJSON())).toContain('Prøv igen');
  expect(calls).toEqual(['delivery_status']);
  await act(async () => { renderer!.unmount(); });
});

test('only an explicit approval queues the visible summary to the selected target', async () => {
  const calls: { command: string; args?: Record<string, unknown> }[] = [];
  const request = async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    if (command === 'delivery_status') return {
      policy: { local_only: false, private: false },
      targets: [{ id: 'one', name: 'Arkiv', destination: { kind: 'folder', path: '/archive' } }], jobs: [],
    } as T;
    return 'queued-id' as T;
  };
  let renderer: ReturnType<typeof create>;
  await act(async () => { renderer = create(<DeliveryApproval meetingId="meeting-a" disabled={false} getSummary={async () => 'Godkendt referat'} request={request} />); });
  expect(renderer!.root.findByType('button').props.disabled).toBe(true);
  await act(async () => { renderer!.root.findByType('input').props.onChange({ target: { checked: true } }); });
  expect(renderer!.root.findByType('button').props.disabled).toBe(false);
  expect(calls).toHaveLength(1);
  await act(async () => { renderer!.root.findByType('button').props.onClick(); });
  expect(calls[1]).toEqual({ command: 'delivery_approve', args: { targetId: 'one', meetingId: 'meeting-a', summary: 'Godkendt referat', private: false } });
  expect(JSON.stringify(renderer!.toJSON())).toContain('lagt i afleveringskøen');
  await act(async () => { renderer!.unmount(); });
});
