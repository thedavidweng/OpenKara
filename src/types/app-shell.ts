export const appShellCopy = {
  summary:
    "Desktop shell initialized with Tauri 2, React, TypeScript, Vite, and Tailwind CSS 4. The repository is now ready for library, playback, separation, and lyrics work.",
  stack: ["Tauri 2", "React 19", "TypeScript 5", "Vite 7", "Tailwind CSS 4"],
  checkpoints: [
    {
      label: "Shell",
      detail: "The desktop window boots against a minimal but real frontend entrypoint.",
    },
    {
      label: "Tooling",
      detail: "Repo-level lint and format commands are in place for follow-up commits.",
    },
    {
      label: "Pathing",
      detail: "The '@/...' alias resolves from Vite and TypeScript, and this screen uses it.",
    },
  ],
} as const;
