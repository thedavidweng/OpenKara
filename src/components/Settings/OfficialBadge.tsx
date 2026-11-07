import { useTranslation } from "react-i18next";

export function OfficialBadge() {
  const { t } = useTranslation();
  return (
    <span className="rounded-full border border-[var(--color-accent)] px-2 py-px text-[10px] font-medium text-[var(--color-accent)]">
      {t("settings.streaming.official")}
    </span>
  );
}
