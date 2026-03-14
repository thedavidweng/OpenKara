import { useRef, type ReactNode } from "react";
import { useLibraryStore } from "@/stores/library-store";

interface ImportButtonProps {
  children: ReactNode;
}

const AUDIO_EXTENSIONS = ".mp3,.flac,.wav,.ogg,.m4a,.aac,.wma";

export function ImportButton({ children }: ImportButtonProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const importFiles = useLibraryStore((s) => s.importFiles);

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files || files.length === 0) return;

    // In Tauri webview, File objects have a `path` property with the real filesystem path
    const paths: string[] = [];
    for (let i = 0; i < files.length; i++) {
      const file = files[i] as File & { path?: string };
      if (file.path) {
        paths.push(file.path);
      }
    }

    if (paths.length > 0) {
      importFiles(paths);
    }

    // Reset so re-selecting the same files triggers onChange
    e.target.value = "";
  };

  return (
    <>
      <button onClick={() => inputRef.current?.click()}>{children}</button>
      <input
        ref={inputRef}
        type="file"
        multiple
        accept={AUDIO_EXTENSIONS}
        onChange={handleChange}
        className="hidden"
      />
    </>
  );
}
