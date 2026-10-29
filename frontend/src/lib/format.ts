// Danske dato- og tidsformater. Al visning af datoer går gennem disse funktioner.

const dateFormat = new Intl.DateTimeFormat('da-DK', { day: 'numeric', month: 'short', year: 'numeric' });
const timeFormat = new Intl.DateTimeFormat('da-DK', { hour: '2-digit', minute: '2-digit' });
const dayFormat = new Intl.DateTimeFormat('da-DK', { weekday: 'long', day: 'numeric', month: 'long' });

/** "29. okt. 2026" */
export function formatDate(date: Date | string | number): string {
  return dateFormat.format(new Date(date));
}

/** "20.31" */
export function formatTime(date: Date | string | number): string {
  return timeFormat.format(new Date(date));
}

/** "torsdag 29. oktober" */
export function formatDayLong(date: Date | string | number): string {
  return dayFormat.format(new Date(date));
}

/** Standardnavn til et nyt møde: "Møde 29. okt. 2026 20.31" */
export function defaultMeetingTitle(date: Date = new Date()): string {
  return `Møde ${formatDate(date)} ${formatTime(date)}`;
}

/** Varighed i sekunder som "1 t 05 min" eller "12 min" eller "45 sek." */
export function formatDuration(totalSeconds: number): string {
  const seconds = Math.max(0, Math.round(totalSeconds));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours > 0) return `${hours} t ${String(minutes).padStart(2, '0')} min`;
  if (minutes > 0) return `${minutes} min`;
  return `${seconds} sek.`;
}
