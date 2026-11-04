export type Policy = { local_only: boolean; private: boolean };
export type Destination =
  | { kind: 'folder'; path: string }
  | { kind: 'webhook'; format: 'json' | 'slack' | 'discord' | 'teams' }
  | { kind: 'ssh'; host: string; port: number; user: string; key_path: string; remote_dir: string };
export type Target = { id: string; name: string; destination: Destination };
export type Job = { id: string; target_id: string; status: string; attempts: number; next_attempt: number; error: string | null };
export type Status = { policy: Policy; targets: Target[]; jobs: Job[] };
export type Request = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export const control = 'min-h-[44px] w-full rounded-md border border-input bg-background px-3 text-base text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring';
export const statusLabel: Record<string, string> = { pending: 'Venter på aflevering', running: 'Afleverer', completed: 'Afleveret', failed: 'Kræver handling' };
