export function createVirtualRowMeasure(
  userAgent: string,
): ((element: Element) => number) | undefined {
  if (/firefox/i.test(userAgent)) {
    return undefined;
  }
  return (element) => element.getBoundingClientRect().height;
}

export function resolveSongListMeasureElement(
  hasWindow: boolean,
  userAgent: string,
): ((element: Element) => number) | undefined {
  if (!hasWindow) {
    return undefined;
  }
  return createVirtualRowMeasure(userAgent);
}
