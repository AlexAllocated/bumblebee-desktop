/** Keep a readable bubble inside both large OBS sources and narrow desktop previews. */
export function bubbleWidthLimit(viewportWidth: number) {
  return Math.max(
    0,
    Math.min(viewportWidth - 16, Math.max(320, viewportWidth * 0.5)),
  );
}

/** The bubble sits 36% above the bottom; leave a gutter above the remaining space. */
export function bubbleHeightLimit(viewportHeight: number) {
  return Math.max(0, viewportHeight * 0.64 - 8);
}

/** Scroll the bubble itself, never the desktop page or the OBS document. */
export function wordScrollTop(
  current: number,
  visibleHeight: number,
  contentHeight: number,
  wordTop: number,
  wordHeight: number,
) {
  const padding = Math.min(8, visibleHeight / 2);
  let next = current;
  if (wordTop < current + padding || wordHeight > visibleHeight - padding * 2) {
    next = wordTop - padding;
  } else if (wordTop + wordHeight > current + visibleHeight - padding) {
    next = wordTop + wordHeight - visibleHeight + padding;
  }
  return Math.max(
    0,
    Math.min(Math.max(0, contentHeight - visibleHeight), next),
  );
}

export function bubbleCenter(
  preferred: number,
  measuredWidth: number,
  viewportWidth: number,
) {
  const half = Math.min(measuredWidth, bubbleWidthLimit(viewportWidth)) / 2;
  const gutter = Math.min(8, viewportWidth / 2);
  return Math.min(
    viewportWidth - gutter - half,
    Math.max(gutter + half, preferred),
  );
}

/** Timing tokens need not include the original spaces/punctuation. Never reconstruct the prose. */
export function wordFragments(
  text: string,
  words: readonly { text: string }[],
) {
  const fragments: { text: string; word: number | null }[] = [];
  let cursor = 0;
  for (const [word, token] of words.entries()) {
    if (!token.text) continue;
    const start = text.indexOf(token.text, cursor);
    if (start < 0) continue;
    if (start > cursor)
      fragments.push({ text: text.slice(cursor, start), word: null });
    fragments.push({
      text: text.slice(start, start + token.text.length),
      word,
    });
    cursor = start + token.text.length;
  }
  if (cursor < text.length)
    fragments.push({ text: text.slice(cursor), word: null });
  return fragments;
}
