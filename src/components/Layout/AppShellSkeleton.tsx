export function AppShellSkeleton() {
  return (
    <div
      data-testid="app-shell-skeleton"
      aria-hidden="true"
      className="flex h-screen w-full bg-[var(--color-surface)]"
    >
      <div className="hidden w-[240px] shrink-0 flex-col gap-3 border-r border-[var(--color-border)] bg-[var(--color-sidebar)] p-4 sm:flex">
        <div className="h-7 w-2/3 rounded-md bg-[var(--color-border)]/50" />
        <div className="mt-2 space-y-2">
          <div className="h-4 w-1/2 rounded bg-[var(--color-border)]/40" />
          <div className="h-4 w-3/4 rounded bg-[var(--color-border)]/30" />
          <div className="h-4 w-2/3 rounded bg-[var(--color-border)]/30" />
        </div>
      </div>

      <div className="flex min-w-0 flex-1 flex-col">
        <div className="h-14 shrink-0 border-b border-[var(--color-border)]" />
        <div className="flex-1" />
        <div className="h-[84px] shrink-0 border-t border-[var(--color-border)] bg-[var(--color-sidebar)]" />
      </div>
    </div>
  );
}
