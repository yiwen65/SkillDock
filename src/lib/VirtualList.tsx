import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";

export function VirtualList<T>({
  className,
  empty,
  estimateSize,
  itemKey,
  items,
  overscan = 8,
  renderItem,
}: {
  className: string;
  empty?: ReactNode;
  estimateSize: number;
  itemKey: (item: T) => string;
  items: T[];
  overscan?: number;
  renderItem: (item: T, index: number) => ReactNode;
}) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);

  useEffect(() => {
    const node = scrollRef.current;
    if (!node) return;

    const updateViewport = () => {
      setViewportHeight(node.clientHeight);
      setScrollTop(node.scrollTop);
    };

    updateViewport();
    const observer = new ResizeObserver(updateViewport);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const node = scrollRef.current;
    if (!node) return;
    node.scrollTop = 0;
    setScrollTop(0);
  }, [items]);

  const range = useMemo(() => {
    if (items.length === 0) {
      return { end: 0, start: 0 };
    }
    const visibleEnd = viewportHeight > 0 ? scrollTop + viewportHeight : estimateSize * overscan;
    const start = Math.max(0, Math.floor(scrollTop / estimateSize) - overscan);
    const end = Math.min(items.length, Math.ceil(visibleEnd / estimateSize) + overscan);
    return { end, start };
  }, [estimateSize, items.length, overscan, scrollTop, viewportHeight]);

  const visibleItems = items.slice(range.start, range.end);

  return (
    <div
      className={`${className} virtual-list`}
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
      ref={scrollRef}
    >
      {items.length === 0 ? (
        empty
      ) : (
        <div className="virtual-list-spacer" style={{ height: items.length * estimateSize }}>
          {visibleItems.map((item, visibleIndex) => {
            const index = range.start + visibleIndex;
            return (
              <div
                className="virtual-list-item"
                key={itemKey(item)}
                style={{ transform: `translateY(${index * estimateSize}px)` }}
              >
                {renderItem(item, index)}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
