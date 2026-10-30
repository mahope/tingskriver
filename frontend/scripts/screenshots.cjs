#!/usr/bin/env node
/**
 * Tager skærmbilleder af den statiske frontend (out/) med mock-data, uden Tauri.
 *
 *   pnpm build
 *   PLAYWRIGHT_PATH=/sti/til/node_modules/playwright node scripts/screenshots.cjs [udmappe]
 *
 * Playwright er ikke en afhængighed i repoet; peg på en eksisterende installation.
 */
const http = require('http');
const fs = require('fs');
const path = require('path');

const playwrightPath = process.env.PLAYWRIGHT_PATH || 'playwright';
const { chromium } = require(playwrightPath);

const outDir = path.resolve(process.argv[2] || path.join(__dirname, '..', '..', 'docs', 'screenshots'));
const siteDir = path.join(__dirname, '..', 'out');
fs.mkdirSync(outDir, { recursive: true });

const MIME = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.json': 'application/json', '.ico': 'image/x-icon', '.txt': 'text/plain' };

function serve() {
  return new Promise((resolve) => {
    const server = http.createServer((req, res) => {
      const urlPath = decodeURIComponent(req.url.split('?')[0]);
      const candidates = [urlPath, urlPath + '.html', path.join(urlPath, 'index.html')];
      for (const c of candidates) {
        const file = path.join(siteDir, c);
        if (file.startsWith(siteDir) && fs.existsSync(file) && fs.statSync(file).isFile()) {
          res.writeHead(200, { 'content-type': MIME[path.extname(file)] || 'application/octet-stream' });
          return fs.createReadStream(file).pipe(res);
        }
      }
      res.writeHead(404);
      res.end('not found');
    });
    server.listen(0, '127.0.0.1', () => resolve(server));
  });
}

// --- Mock-data ---------------------------------------------------------------
const now = new Date();
const at = (daysAgo, h, m) => new Date(now.getFullYear(), now.getMonth(), now.getDate() - daysAgo, h, m).toISOString();
const MEETINGS = [
  { id: 'm-0001-uge', title: 'Ugentligt statusmøde', created_at: at(0, 9, 30) },
  { id: 'm-0002-kunde', title: 'Kickoff med Havnens Bageri', created_at: at(0, 13, 5) },
  { id: 'm-0003-plan', title: 'Planlægning af efterårskampagne', created_at: at(1, 10, 0) },
  { id: 'm-0004-retro', title: 'Retrospektiv: sidste sprint', created_at: at(3, 14, 15) },
];
const SUMMARY_MD = `## Kort fortalt

Teamet gennemgik status på de tre åbne spor og blev enige om at udskyde lanceringen af den nye bookingside til efter weekenden. Kundens feedback på prototypen var overvejende positiv.

## Beslutninger

- Lanceringen af bookingsiden flyttes til tirsdag, så testen når at blive færdig.
- Vi bruger den eksisterende betalingsløsning i første omgang.
- Næste statusmøde holdes fredag kl. 9.30.

## Handlepunkter

- Maria sender prøveadgang til kunden inden onsdag.
- Jonas retter mobilvisningen af kalenderen.
- Sofie booker lokale til workshoppen i november.

## Åbne spørgsmål

- Skal ventelisten have sin egen bekræftelsesmail?
- Hvem har ansvaret for opdatering af priserne?
`;
const TRANSCRIPT_LINES = [
  'Godmorgen alle sammen, så går vi i gang med ugens status.',
  'Jeg starter med bookingsiden. Vi er tæt på, men testen er ikke helt færdig.',
  'Kan vi nå det til fredag, eller skal vi rykke lanceringen?',
  'Jeg tror, det bliver tirsdag, hvis vi vil være sikre på mobilvisningen.',
  'Det er fint for mig. Kunden har alligevel ikke travlt før næste uge.',
  'Så beslutter vi det: lanceringen flyttes til tirsdag.',
  'Maria, kan du sende prøveadgang til kunden senest onsdag?',
  'Ja, det klarer jeg i dag.',
  'Og så skal vi huske at booke lokale til workshoppen i november.',
  'Det tager jeg, siger Sofie.',
];
const TRANSCRIPTS = TRANSCRIPT_LINES.map((text, i) => ({
  id: `t-${i}`, text, timestamp: at(0, 9, 30 + i), audio_start_time: i * 19 + 3, audio_end_time: i * 19 + 15, duration: 12,
  confidence: i === 4 ? 0.3 : 0.9,
}));
const LIVE_LINES = TRANSCRIPTS.slice(0, 5);
const DICTATIONS = [
  { id: 6, createdAt: Date.parse(at(0, 14, 2)), text: 'Hej Anna, tak for i dag. Jeg sender tilbuddet i morgen formiddag.', app: 'Spark.exe', style: 'normal', engine: 'syv', fallbackReason: null, latencyMs: 940, audioMs: 5200, insertMethod: 'paste', hasAudio: true },
  { id: 5, createdAt: Date.parse(at(0, 13, 48)), text: 'git commit -m "ret hotswap"', app: 'WindowsTerminal.exe', style: 'raw', engine: 'syv', fallbackReason: null, latencyMs: 710, audioMs: 2600, insertMethod: 'paste', hasAudio: true },
  { id: 4, createdAt: Date.parse(at(0, 11, 15)), text: 'Kan vi rykke mødet til torsdag kl. 10? Jeg har en kunde om onsdagen.', app: 'ms-teams.exe', style: 'normal', engine: 'local', fallbackReason: 'Syv.ai svarede ikke inden for 2000 ms', latencyMs: 2380, audioMs: 4100, insertMethod: 'paste', hasAudio: true },
  { id: 3, createdAt: Date.parse(at(1, 16, 30)), text: 'Husk at opdatere prislisten.\nSend den til Mahoje bagefter.', app: 'chrome.exe', style: 'normal', engine: 'syv', fallbackReason: null, latencyMs: 1120, audioMs: 6800, insertMethod: 'paste', hasAudio: false },
];
const DICTATION_SETTINGS = {
  enabled: true, shortcut: 'Ctrl+Win', engine: 'auto', syvEndpoint: 'https://platform.syv.ai/v1', syvModel: 'syv-transcribe',
  fallbackAfterMs: 2000, polishEnabled: false, polishProvider: 'builtin-ai', polishModel: '', polishEndpoint: '', polishBudgetMs: 2000,
  historyEnabled: true, keepAudio: true, restoreClipboardMs: 300,
  appStyles: ['Code.exe', 'WindowsTerminal.exe', 'pwsh.exe', 'cmd.exe', 'idea64.exe'].map((process) => ({ process, style: 'raw' })),
};

function mockScript(scene) {
  return `(() => {
  const scene = ${JSON.stringify(scene)};
  const MEETINGS = ${JSON.stringify(MEETINGS)};
  const TRANSCRIPTS = ${JSON.stringify(TRANSCRIPTS)};
  const LIVE = ${JSON.stringify(LIVE_LINES)};
  const DICTATIONS = ${JSON.stringify(DICTATIONS)};
  const DICTATION_SETTINGS = ${JSON.stringify(DICTATION_SETTINGS)};
  const SUMMARY = ${JSON.stringify({ markdown: SUMMARY_MD })};
  const callbacks = {}; let nextId = 1; const listeners = {};
  window.__calls = [];
  const recording = scene.recording;
  const handlers = {
    get_onboarding_status: () => scene.onboarding
      ? { version: '1.0', completed: false, current_step: scene.onboardingStep || 1, model_status: { parakeet: 'not_downloaded', summary: 'not_downloaded' }, last_updated: new Date().toISOString() }
      : { version: '1.0', completed: true, current_step: 4, model_status: { parakeet: 'downloaded', summary: 'downloaded' }, last_updated: new Date().toISOString() },
    api_get_meetings: () => scene.emptyList ? [] : MEETINGS,
    api_get_meeting_metadata: (a) => { const m = MEETINGS.find(x => x.id === a.meetingId) || MEETINGS[0]; return { id: m.id, title: m.title, created_at: m.created_at, updated_at: m.created_at, folder_path: 'C:/Optagelser/' + m.id }; },
    api_get_meeting_transcripts: () => ({ transcripts: scene.noTranscript ? [] : TRANSCRIPTS, total_count: scene.noTranscript ? 0 : TRANSCRIPTS.length, has_more: false }),
    api_get_meeting: (a) => { const m = MEETINGS.find(x => x.id === a.meetingId) || MEETINGS[0]; return { id: m.id, title: m.title, created_at: m.created_at, updated_at: m.created_at, transcripts: TRANSCRIPTS }; },
    api_get_summary: () => scene.noSummary ? { status: 'idle', meetingName: null, meeting_id: 'x', start: null, end: null, data: null, error: null } : { status: 'completed', meetingName: 'x', meeting_id: 'x', start: null, end: null, data: SUMMARY, error: null },
    api_get_model_config: () => ({ provider: 'builtin-ai', model: 'gemma3:4b', whisperModel: 'large-v3', apiKey: null, ollamaEndpoint: null }),
    api_get_transcript_config: () => ({ provider: 'parakeet', model: 'parakeet-tdt-0.6b-v3-int8', apiKey: null }),
    is_recording: () => !!recording,
    get_recording_state: () => ({ is_recording: !!recording, is_paused: !!scene.paused, is_active: !!recording && !scene.paused, recording_duration: recording ? 754 : null, active_duration: recording ? 754 : null }),
    get_recording_meeting_name: () => recording ? 'Møde 29. okt. 2026 20.31' : null,
    'plugin:app|version': () => '0.5.0-dev.2',
    'plugin:event|listen': (a) => { const id = nextId++; (listeners[a.event] = listeners[a.event] || []).push({ id, handler: a.handler }); return id; },
    'plugin:event|unlisten': () => null,
    'plugin:path|resolve_directory': () => 'C:/Users/Mads/AppData/Roaming/dk.mahoje.tingskriver',
    get_audio_devices: () => [{ name: 'Headset (USB)', device_type: 'Input' }, { name: 'Højttalere (Realtek)', device_type: 'Output' }],
    list_audio_devices: () => [{ name: 'Headset (USB)', device_type: 'Input' }, { name: 'Højttalere (Realtek)', device_type: 'Output' }],
    get_ollama_models: () => [],
    api_list_templates: () => [{ id: 'standard_meeting', name: 'Standardmøde', description: 'Beslutninger og handlepunkter' }, { id: 'daily_standup', name: 'Daglig stand-up', description: 'Kort status' }],
    'plugin:store|load': () => 1,
    'plugin:store|get': () => [null, false],
    'plugin:store|set': () => null,
    'plugin:store|save': () => null,
    'plugin:store|get_store': () => 1,
    check_audio_permissions: () => ({ hasMicrophone: true, hasSystemAudio: true }),
    get_transcript_history: () => [],
    dictation_get_settings: () => DICTATION_SETTINGS,
    dictation_get_status: () => ({ platformSupported: true, shortcutActive: true, hasSyvKey: true, localModelAvailable: true, lastError: null }),
    dictation_list_history: () => scene.emptyList ? [] : DICTATIONS,
    dictation_list_dictionary: () => [{ id: 1, word: 'Mahoje', aliases: 'maho je, mahoye' }, { id: 2, word: 'Syv.ai', aliases: 'syv ai' }, { id: 3, word: 'Tingskriver', aliases: '' }],
    dictation_list_snippets: () => [{ id: 1, phrase: 'mit cvr', text: 'CVR 39413892' }, { id: 2, phrase: 'booking-link', text: 'Book et møde her: https://mahoje.dk/book' }],
  };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } },
    transformCallback: (cb, once) => { const id = nextId++; callbacks[id] = cb; window['_' + id] = cb; return id; },
    unregisterCallback: (id) => { delete callbacks[id]; },
    convertFileSrc: (p) => p,
    invoke: async (cmd, args) => {
      window.__calls.push(cmd);
      const h = handlers[cmd];
      if (h) return h(args || {});
      return null;
    },
  };
  window.__TAURI_OS_PLUGIN_INTERNALS__ = { os_type: 'windows', platform: 'windows', family: 'windows', version: '10.0.26100', arch: 'x86_64', exe_extension: 'exe' };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  window.__emit = (event, payload) => { (listeners[event] || []).forEach(l => { const cb = callbacks[l.handler]; if (cb) cb({ event, id: l.id, payload }); }); };
  localStorage.setItem('tingskriver.theme', scene.theme);
  if (scene.recording) {
    window.addEventListener('load', () => setTimeout(() => {
      LIVE.forEach((t, i) => window.__emit('transcript-update', { text: t.text, timestamp: t.timestamp, audio_start_time: t.audio_start_time, audio_end_time: t.audio_end_time, confidence: t.confidence, sequence_id: i, is_partial: false, source_type: 'user', chunk_start_time: t.audio_start_time, is_partial: false }));
      if (scene.deviceNotice) window.__emit('mic-device-switched', { device_name: 'Headset (USB)' });
    }, 1200));
  }
})();`;
}

const SCENES = [
  { name: 'moedeliste', path: '/', emptyList: false },
  { name: 'moedeside', path: '/meeting-details?id=m-0001-uge' },
  { name: 'moedeside-transskript', path: '/meeting-details?id=m-0001-uge', openTranscript: true },
  { name: 'moedeside-uden-opsummering', path: '/meeting-details?id=m-0002-kunde', noSummary: true },
  { name: 'optagelse-i-gang', path: '/', recording: true },
  { name: 'optagelse-enhed-skiftet', path: '/', recording: true, deviceNotice: true },
  { name: 'optagelse-pause', path: '/', recording: true, paused: true },
  { name: 'indstillinger', path: '/settings' },
  { name: 'onboarding', path: '/', onboarding: true, onboardingStep: 1 },
  { name: 'onboarding-model', path: '/', onboarding: true, onboardingStep: 3 },
  { name: 'tom-tilstand', path: '/', emptyList: true },
  { name: 'diktering', path: '/diktering' },
  { name: 'diktering-tom', path: '/diktering', emptyList: true },
  { name: 'indstillinger-diktering', path: '/settings?tab=diktering', viewport: { width: 1280, height: 2850 } },
  { name: 'diktering-pille', path: '/dictation-overlay.html?state=recording&demo=1', viewport: { width: 220, height: 52 }, overlay: true },
  { name: 'diktering-pille-skriver', path: '/dictation-overlay.html?state=processing', viewport: { width: 220, height: 52 }, overlay: true },
];

(async () => {
  const server = await serve();
  const base = `http://127.0.0.1:${server.address().port}`;
  const browser = await chromium.launch();
  const only = process.env.ONLY ? process.env.ONLY.split(',') : null;
  for (const theme of ['light', 'dark']) {
    for (const scene of SCENES) {
      if (only && !only.includes(scene.name)) continue;
      // The overlay pill is always dark; one screenshot is enough.
      if (scene.overlay && theme === 'dark') continue;
      const ctx = await browser.newContext({ viewport: scene.viewport || { width: 1280, height: 800 }, colorScheme: theme, deviceScaleFactor: scene.overlay ? 2 : 1, locale: 'da-DK' });
      await ctx.addInitScript(mockScript({ ...scene, theme }));
      const page = await ctx.newPage();
      const errors = [];
      page.on('pageerror', (e) => errors.push(String(e)));
      page.on('console', (m) => { if (process.env.DEBUG_CONSOLE && m.type() === 'error') console.log('   [console]', m.text().slice(0, 300)); });
      await page.goto(base + scene.path, { waitUntil: 'networkidle' });
      await page.waitForTimeout(scene.recording ? 2500 : 1500);
      if (scene.openTranscript) {
        await page.getByRole('button', { name: /Transskript/ }).first().click();
        await page.waitForTimeout(600);
        await page.getByRole('button', { name: /Transskript/ }).first().scrollIntoViewIfNeeded();
      }
      const file = path.join(outDir, scene.overlay ? `${scene.name}.png` : `${scene.name}-${theme === 'light' ? 'lys' : 'moerk'}.png`);
      await page.screenshot({ path: file, omitBackground: !!scene.overlay });
      console.log('✔', path.basename(file), errors.length ? `(${errors.length} sidefejl: ${errors[0].slice(0, 120)})` : '');
      await ctx.close();
    }
  }
  await browser.close();
  server.close();
})();
