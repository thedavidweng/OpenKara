const coverModules = import.meta.glob<string>("../../src/mock/covers/*.jpg", {
  eager: true,
  query: "?url",
  import: "default",
});

export const PREVIEW_COVER_URLS: Record<string, string> = Object.fromEntries(
  Object.entries(coverModules).map(([path, url]) => {
    const file = path.slice(path.lastIndexOf("/") + 1);
    return [file.slice(0, -".jpg".length), url];
  }),
);
