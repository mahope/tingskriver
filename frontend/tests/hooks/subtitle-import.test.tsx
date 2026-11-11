import { afterAll, describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

const originalCore = { ...await import('@tauri-apps/api/core') };
const originalNavigation = { ...await import('next/navigation') };
const originalConfig = { ...await import('../../src/contexts/ConfigContext') };
const originalSidebar = { ...await import('../../src/components/Sidebar/SidebarProvider') };
const originalToast = { ...await import('sonner') };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('next/navigation', () => originalNavigation);
  mock.module('../../src/contexts/ConfigContext', () => originalConfig);
  mock.module('../../src/components/Sidebar/SidebarProvider', () => originalSidebar);
  mock.module('sonner', () => originalToast);
});

const push = mock(() => {});
const refetchMeetings = mock(async () => {});
let summaryError: string | null = null;
mock.module('next/navigation', () => ({ useRouter: () => ({ push }) }));
mock.module('../../src/contexts/ConfigContext', () => ({ useConfig: () => ({ betaFeatures: {} }) }));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({ useSidebar: () => ({ refetchMeetings }) }));
mock.module('sonner', () => ({ toast: { info() {}, error() {}, success() {} } }));
mock.module('@tauri-apps/api/core', () => ({
  ...originalCore,
  invoke: async () => ({ meeting_id: 'imported', segments_count: 2, already_imported: false, summary_error: summaryError }),
}));
const { ImportDialogProvider, useImportDialog } = await import('../../src/contexts/ImportDialogContext');
let importTranscript: ReturnType<typeof useImportDialog>['importTranscript'];
function Consumer() {
  importTranscript = useImportDialog().importTranscript;
  return null;
}

describe('subtitle import meeting list', () => {
  for (const error of [null, 'No summary provider configured']) {
    test(`refreshes saved meetings before navigation (summary error: ${error})`, async () => {
      summaryError = error;
      refetchMeetings.mockClear();
      push.mockClear();
      let renderer!: ReturnType<typeof create>;
      await act(async () => { renderer = create(<ImportDialogProvider onOpen={() => {}}><Consumer /></ImportDialogProvider>); });
      try {
        await act(async () => { await importTranscript('transcript.vtt'); });
        expect(refetchMeetings).toHaveBeenCalledTimes(1);
        expect(push).toHaveBeenCalledWith('/meeting-details?id=imported');
        expect(refetchMeetings.mock.invocationCallOrder[0]).toBeLessThan(push.mock.invocationCallOrder[0]);
      } finally {
        renderer.unmount();
      }
    });
  }
});
