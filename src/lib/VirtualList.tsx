import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";

function findItemIndex(offsets: number[], value: number) {
  let low = 0;
  let high = offsets.length - 1;

  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if (offsets[mid] <= value) {
      low = mid + 1;
    } else {
      high = mid;
    }
  }

  return Math.max(0, low - 1);
}

function VirtualListItem({
  children,
  itemId,
  offset,
  onMeasure,
}: {
  children: ReactNode;
  itemId: string;
  offset: number;
  onMeasure: (itemId: string, size: number) => void;
}) {
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const node = ref.current;
    if (!node) return undefined;

    const updateMeasuredSize = () => {
      onMeasure(itemId, Math.ceil(node.getBoundingClientRect().height));
    };

    updateMeasuredSize();
    const observer = new ResizeObserver(updateMeasuredSize);
    observer.observe(node);
    return () => observer.disconnect();
  }, [itemId, onMeasure]);

  return (
    <div className="virtual-list-item" ref={ref} style={{ transform: `translateY(${offset}px)` }}>
      {children}
    </div>
  );
}

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
  const measuredSizesRef = useRef(new Map<string, number>());
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [measurementVersion, setMeasurementVersion] = useState(0);

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

  const offsets = useMemo(() => {
    const nextOffsets = new Array<number>(items.length + 1);
    nextOffsets[0] = 0;
    items.forEach((item, index) => {
      const measuredSize = measuredSizesRef.current.get(itemKey(item));
      nextOffsets[index + 1] = nextOffsets[index] + (measuredSize ?? estimateSize);
    });
    return nextOffsets;
  }, [estimateSize, itemKey, items, measurementVersion]);

  const totalHeight = offsets[offsets.length - 1] ?? 0;

  const range = useMemo(() => {
    if (items.length === 0) {
      return { end: 0, start: 0 };
    }
    const visibleEnd = viewportHeight > 0 ? scrollTop + viewportHeight : estimateSize * overscan;
    const start = Math.max(0, findItemIndex(offsets, scrollTop) - overscan);
    const end = Math.min(items.length, findItemIndex(offsets, visibleEnd) + overscan + 1);
    return { end, start };
  }, [estimateSize, items.length, offsets, overscan, scrollTop, viewportHeight]);

  const visibleItems = items.slice(range.start, range.end);

  const measureItem = useCallback((key: string, nextSize: number) => {
    const currentSize = measuredSizesRef.current.get(key);
    if (nextSize > 0 && currentSize !== nextSize) {
      measuredSizesRef.current.set(key, nextSize);
      setMeasurementVersion((version) => version + 1);
    }
  }, []);

  return (
    <div
      className={`${className} virtual-list`}
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
      ref={scrollRef}
    >
      {items.length === 0 ? (
        empty
      ) : (
        <div className="virtual-list-spacer" style={{ height: totalHeight }}>
          {visibleItems.map((item, visibleIndex) => {
            const index = range.start + visibleIndex;
            const key = itemKey(item);
            return (
              <VirtualListItem
                itemId={key}
                key={key}
                offset={offsets[index]}
                onMeasure={measureItem}
              >
                {renderItem(item, index)}
              </VirtualListItem>
            );
          })}
        </div>
      )}
    </div>
  );
}
