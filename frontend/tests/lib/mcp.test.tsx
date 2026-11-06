import { expect, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { McpSettings, MeetingSharing } from '../../src/components/mcp/McpSettings';

test('MCP defaults to marked meetings, saves explicitly and copies exact configuration', async () => {
  const calls: { command: string; args?: Record<string, unknown> }[] = [];
  const request = async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    return { access: 'marked', claude: 'claude mcp add fixture', codex: '[mcp_servers.fixture]' } as T;
  };
  const copied: string[] = [];
  let view: ReturnType<typeof create>;
  await act(async () => { view = create(<McpSettings request={request} copy={async text => { copied.push(text); }} />); });
  expect(view!.root.findByType('select').props.value).toBe('marked');
  expect(JSON.stringify(view!.toJSON())).toContain('forlader computeren');
  await act(async () => { view!.root.findByType('select').props.onChange({ target: { value: 'all' } }); });
  expect(calls[1]).toEqual({ command: 'mcp_set_access', args: { access: 'all' } });
  await act(async () => { view!.root.findAllByType('button')[0].props.onClick(); });
  expect(copied).toEqual(['claude mcp add fixture']);
  await act(async () => { view!.unmount(); });
});

test('meeting sharing starts disabled and failed writes preserve the saved value', async () => {
  const request = async <T,>(command: string): Promise<T> => {
    if (command === 'mcp_meeting_shared') return false as T;
    throw new Error('fixture');
  };
  let view: ReturnType<typeof create>;
  await act(async () => { view = create(<MeetingSharing id="fixture" request={request} />); });
  expect(view!.root.findByType('input').props.checked).toBe(false);
  await act(async () => { view!.root.findByType('input').props.onChange({ target: { checked: true } }); });
  expect(view!.root.findByType('input').props.checked).toBe(false);
  expect(view!.root.findByProps({ role: 'alert' }).children.join('')).toContain('kunne ikke gemmes');
  await act(async () => { view!.unmount(); });
});
