import { useEffect, useRef, type KeyboardEvent, type PointerEvent } from "react";

export interface SplitterProps {
  readonly value: number;
  readonly min: number;
  readonly max: number;
  readonly step: number;
  readonly label: string;
  /** "vertical" (default) drags along x; "horizontal" is a bar between stacked panes and drags along y. */
  readonly orientation?: "vertical" | "horizontal";
  readonly onResize: (value: number) => void;
  readonly onToggle: () => void;
}

interface DragStart {
  readonly x: number;
  readonly value: number;
}

/** Vertical splitter: pointer drag and keyboard resizing. */
export function Splitter({ value, min, max, step, label, orientation = "vertical", onResize, onToggle }: SplitterProps) {
  const dragRef = useRef<DragStart | null>(null);
  const cleanupRef = useRef<(() => void) | null>(null);
  const onResizeRef = useRef(onResize);
  onResizeRef.current = onResize;

  useEffect(() => () => cleanupRef.current?.(), []);

  const horizontal = orientation === "horizontal";
  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    dragRef.current = { x: horizontal ? event.clientY : event.clientX, value };
    event.currentTarget.setPointerCapture?.(event.pointerId);
    const move = (e: globalThis.PointerEvent) => {
      const start = dragRef.current;
      if (start) onResizeRef.current(start.value + ((horizontal ? e.clientY : e.clientX) - start.x));
    };
    const stop = () => {
      dragRef.current = null;
      cleanupRef.current?.();
      cleanupRef.current = null;
    };
    cleanupRef.current = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const next: Record<string, () => void> = {
      [horizontal ? "ArrowUp" : "ArrowLeft"]: () => onResize(value - step),
      [horizontal ? "ArrowDown" : "ArrowRight"]: () => onResize(value + step),
      Home: () => onResize(min),
      End: () => onResize(max),
      Enter: onToggle,
    };
    const action = next[event.key];
    if (!action) return;
    event.preventDefault();
    action();
  };

  return (
    <div
      role="separator"
      aria-orientation={orientation}
      aria-label={label}
      aria-valuenow={value}
      aria-valuemin={min}
      aria-valuemax={max}
      tabIndex={0}
      className={horizontal ? "splitter splitter-horizontal" : "splitter"}
      onPointerDown={handlePointerDown}
      onKeyDown={handleKeyDown}
    />
  );
}
