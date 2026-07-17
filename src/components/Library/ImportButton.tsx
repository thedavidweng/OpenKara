import { type ReactNode } from "react";
import { useLibraryStore } from "@/stores/library-store";
import { promptImportFiles } from "@/runtime/menu-runtime";

interface ImportButtonProps {
  children: ReactNode;
  ariaLabel?: string;
  onClick?: () => void | Promise<void>;
}

export function ImportButton({
  children,
  ariaLabel,
  onClick,
}: ImportButtonProps) {
  const importFiles = useLibraryStore((s) => s.importFiles);

  const handleClick = async () => {
    if (onClick) {
      await onClick();
      return;
    }
    await promptImportFiles({ importFiles });
  };

  return (
    <button onClick={handleClick} aria-label={ariaLabel}>
      {children}
    </button>
  );
}
