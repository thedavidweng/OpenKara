import { useEffect } from "react";
import { AppLayout } from "@/components/Layout/AppLayout";
import { TooltipProvider } from "@/components/Overlay/Tooltip";
import { initializeMockApp } from "./mock-app";

export function AppPreview({ language }: { language: "en" | "zh-CN" }) {
  initializeMockApp(language);

  useEffect(() => {
    initializeMockApp(language);
  }, [language]);

  return (
    <div className="product-preview" aria-label="Interactive OpenKara preview">
      <TooltipProvider>
        <AppLayout previewMode />
      </TooltipProvider>
    </div>
  );
}
