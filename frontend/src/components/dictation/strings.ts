export const strings = {
  shortcutMode: 'Genvejstilstand',
  modes: {
    hold: 'Hold: tal mens tasterne holdes nede',
    toggle: 'Skift: tryk for at starte og stoppe',
    hybrid: 'Hybrid: kort tryk låser, langt tryk bruger hold',
    'double-tap': 'Dobbelttryk-lås: to hurtige tryk låser',
  },
  hybridThreshold: 'Grænse for kort tryk (ms)',
  hybridHelp: '100–1000 ms. Standard er 300 ms. Et kortere tryk låser mikrofonen, indtil du trykker igen.',
} as const;
