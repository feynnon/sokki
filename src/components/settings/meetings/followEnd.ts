import { useCallback, useLayoutEffect, useRef, useState } from "react";

/** How close to the end (px) still counts as "at the end", so a
 * sub-pixel scroll position or a few pixels of overscroll keep following. */
export const END_SLACK = 24;

/** True when a scroll box shows its last line. */
export function atEnd(
  scrollHeight: number,
  scrollTop: number,
  clientHeight: number,
  slack = END_SLACK,
): boolean {
  return scrollHeight - scrollTop - clientHeight <= slack;
}

/**
 * A scroll box that keeps its end in view, like a chat: it opens at the
 * end, follows new content, stops when the person scrolls up to read, and
 * follows again once they scroll back down or call `jump`.
 *
 * `content` changes whenever new content arrives (e.g. the line count).
 */
export function useFollowEnd(content: unknown) {
  const box = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const [reading, setReading] = useState(false);

  const toEnd = () => {
    const b = box.current;
    if (b) b.scrollTop = b.scrollHeight;
  };

  // Before paint, so the first frame already shows the newest line.
  useLayoutEffect(() => {
    if (following.current) toEnd();
  }, [content]);

  // A taller or shorter box (window resized) keeps the end in view too.
  useLayoutEffect(() => {
    const b = box.current;
    if (!b || typeof ResizeObserver === "undefined") return;
    const o = new ResizeObserver(() => {
      if (following.current) toEnd();
    });
    o.observe(b);
    return () => o.disconnect();
  }, []);

  const onScroll = useCallback(() => {
    const b = box.current;
    if (!b) return;
    const end = atEnd(b.scrollHeight, b.scrollTop, b.clientHeight);
    following.current = end;
    setReading(!end);
  }, []);

  const jump = useCallback(() => {
    following.current = true;
    setReading(false);
    toEnd();
  }, []);

  return { box, onScroll, reading, jump };
}
