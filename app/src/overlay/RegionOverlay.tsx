import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface OverlayInfo {
  image: string;
}

interface Point {
  x: number;
  y: number;
}

interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

function normalize(start: Point, end: Point): Rect {
  const x = Math.min(start.x, end.x);
  const y = Math.min(start.y, end.y);
  return {
    x,
    y,
    width: Math.abs(end.x - start.x),
    height: Math.abs(end.y - start.y),
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

export function RegionOverlay() {
  const [image, setImage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [start, setStart] = useState<Point | null>(null);
  const [current, setCurrent] = useState<Point | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const pointerId = useRef<number | null>(null);
  const finished = useRef(false);

  const cancel = useCallback(() => {
    if (finished.current) return;
    finished.current = true;
    void invoke("overlay_cancel").catch(() => undefined);
  }, []);

  useEffect(() => {
    invoke<OverlayInfo>("overlay_info")
      .then((info) => setImage(info.image))
      .catch((cause) => setError(String(cause)));

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        cancel();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [cancel]);

  const selection = start && current ? normalize(start, current) : null;

  function onPointerDown(event: React.PointerEvent<HTMLDivElement>) {
    if (!image || finished.current) return;
    if (event.button !== 0) {
      cancel();
      return;
    }
    event.preventDefault();
    const point = { x: event.clientX, y: event.clientY };
    setNotice(null);
    setStart(point);
    setCurrent(point);
    pointerId.current = event.pointerId;
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: React.PointerEvent<HTMLDivElement>) {
    if (pointerId.current !== event.pointerId || !start) return;
    setCurrent({
      x: clamp(event.clientX, 0, window.innerWidth),
      y: clamp(event.clientY, 0, window.innerHeight),
    });
  }

  function onPointerUp(event: React.PointerEvent<HTMLDivElement>) {
    if (pointerId.current !== event.pointerId || !start) return;
    pointerId.current = null;
    const rect = normalize(start, {
      x: clamp(event.clientX, 0, window.innerWidth),
      y: clamp(event.clientY, 0, window.innerHeight),
    });
    if (rect.width < 4 || rect.height < 4) {
      setStart(null);
      setCurrent(null);
      setNotice("区域太小，请重新拖选");
      return;
    }
    finished.current = true;
    void invoke("overlay_finish", {
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
      scale: window.devicePixelRatio || 1,
    }).catch((cause) => {
      setError(String(cause));
      finished.current = false;
    });
  }

  return (
    <div
      className="region-overlay"
      onContextMenu={(event) => {
        event.preventDefault();
        cancel();
      }}
      onPointerCancel={cancel}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    >
      {image ? (
        <img src={image} alt="" draggable={false} />
      ) : (
        <div className="loading">{error ?? "正在准备截图…"}</div>
      )}

      {!selection && <div className="shade" />}

      {selection && (
        <div
          className="selection"
          style={{
            left: selection.x,
            top: selection.y,
            width: selection.width,
            height: selection.height,
          }}
        >
          <span className="corner corner-tl" />
          <span className="corner corner-tr" />
          <span className="corner corner-bl" />
          <span className="corner corner-br" />
          <span
            className="size"
            style={selection.y < 36 ? { top: 6, left: 6 } : undefined}
          >
            {Math.round(selection.width)} × {Math.round(selection.height)}
          </span>
        </div>
      )}

      {notice && (
        <div className="notice" role="status">
          {notice}
        </div>
      )}

      <div className="hint">
        拖动选择区域 · 右键或 <kbd>Esc</kbd> 取消
      </div>
    </div>
  );
}
