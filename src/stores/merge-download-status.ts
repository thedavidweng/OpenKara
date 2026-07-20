interface DownloadStatus {
  state: string;
  downloaded_bytes?: number | null;
  total_bytes?: number | null;
}

export function mergeDownloadStatus<T extends DownloadStatus>(
  previous: T | null,
  incoming: T,
  pathField: keyof T,
): T {
  if (
    previous &&
    incoming.state === "downloading" &&
    previous.state === "downloading" &&
    incoming[pathField] === previous[pathField]
  ) {
    const prevDown = previous.downloaded_bytes ?? 0;
    const nextDown = Math.max(prevDown, incoming.downloaded_bytes ?? 0);
    const nextTotal = incoming.total_bytes ?? previous.total_bytes ?? null;
    return {
      ...incoming,
      downloaded_bytes: nextDown,
      total_bytes: nextTotal,
    };
  }
  return incoming;
}
