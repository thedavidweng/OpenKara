import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, LogOut, UserRound } from "lucide-react";
import { firstGrapheme } from "@/components/Catalog/streaming-utils";
import { useCatalogStore } from "@/stores/catalog-store";
import { ConfirmationDialog } from "@/components/Settings/ConfirmationDialog";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

export function StreamingAccountMenu({ name }: { name: string }) {
  const { t } = useTranslation();
  const signOut = useCatalogStore((s) => s.signOut);
  const source = useCatalogStore((s) =>
    s.sources.find((item) => item.source_id === s.activeSourceId),
  );
  const [confirmStop, setConfirmStop] = useState(false);
  const displayName = name.trim();

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button
              variant="ghost"
              className="h-9 max-w-64 gap-2 pl-1"
              title={displayName || undefined}
            />
          }
        >
          <span
            aria-hidden="true"
            className="flex size-7 shrink-0 items-center justify-center rounded-full bg-secondary text-[13px] font-semibold"
          >
            {displayName ? (
              firstGrapheme(displayName)
            ) : (
              <UserRound className="size-4" />
            )}
          </span>
          {displayName ? <span className="truncate">{displayName}</span> : null}
          <span className="sr-only">{t("catalog.streaming.accountMenu")}</span>
          <ChevronDown aria-hidden="true" className="text-muted-foreground" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-auto min-w-40">
          <DropdownMenuItem
            onClick={() => {
              if (source?.active_work.length) setConfirmStop(true);
              else void signOut();
            }}
          >
            <LogOut />
            {t("catalog.streaming.signOut")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {confirmStop && source ? (
        <ConfirmationDialog
          title={t("catalog.streaming.signOut")}
          message={t("settings.streaming.stopWorkBody")}
          detail={source.active_work
            .map((work) => t(`settings.streaming.work.${work.kind}`))
            .join(", ")}
          confirmLabel={t("settings.streaming.stopWork")}
          onCancel={() => setConfirmStop(false)}
          onConfirm={() => {
            setConfirmStop(false);
            void signOut(source.source_id, true);
          }}
        />
      ) : null}
    </>
  );
}
