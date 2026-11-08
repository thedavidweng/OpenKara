import { Badge } from "@/components/ui/badge";
import { useTranslation } from "react-i18next";

export function OfficialBadge() {
  const { t } = useTranslation();
  return (
    <Badge
      variant="outline"
      className="border-[var(--color-accent)] text-[var(--color-accent)]"
    >
      {t("settings.streaming.official")}
    </Badge>
  );
}
