export function isPreviewCatalogModule(id: string): boolean {
  const normalized = id.replace(/\\/g, "/").split("?")[0];
  return normalized.endsWith("/src/mock/preview-songs.ts");
}

export function slimPreviewCatalogSource(code: string): string {
  return code
    .replace(/cover_art_base64:\s*"[^"]*"/g, 'cover_art_base64: ""')
    .replace(/raw_lrc:\s*'(?:\\.|[^'\\])*'/g, "raw_lrc: ''")
    .replace(/raw_lrc:\s*"(?:\\.|[^"\\])*"/g, 'raw_lrc: ""');
}
