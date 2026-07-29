import type { ReactNode } from "react";

interface SettingsSectionCardProps {
  title: string;
  description?: string;
  tone?: "default" | "danger";
  children: ReactNode;
}

export function SettingsSectionCard({
  title,
  description,
  tone = "default",
  children,
}: SettingsSectionCardProps) {
  const isDanger = tone === "danger";

  return (
    <section className="space-y-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-5">
      <div className="space-y-1">
        {/*
          Section titles use primary text + semibold so panels scan like Danger
          Zone: hierarchy first, color only when semantic (danger). Dim body
          copy stays on the description line.
        */}
        <label
          className={`text-[13px] font-semibold tracking-tight break-words ${
            isDanger
              ? "text-[var(--color-destructive)]"
              : "text-[var(--color-text)]"
          }`}
        >
          {title}
        </label>
        {description && (
          <p className="text-[12px] text-[var(--color-text-dim)] break-words">
            {description}
          </p>
        )}
      </div>

      {children}
    </section>
  );
}
