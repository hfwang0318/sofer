import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import {
  acceleratorFromEvent,
  isPlatformModifier,
  shortcutKeys,
  shortcutText,
} from "../lib/platform";

interface KeycapsProps {
  accelerator: string;
  size?: "sm" | "md";
  className?: string;
}

export function Keycaps({ accelerator, size = "md", className = "" }: KeycapsProps) {
  return (
    <span
      className={`keycaps ${className}`}
      role="img"
      aria-label={`快捷键：${shortcutText(accelerator)}`}
    >
      {shortcutKeys(accelerator).map((key, index) => (
        <kbd className={`keycap keycap-${size}`} key={`${key}-${index}`}>
          {key}
        </kbd>
      ))}
    </span>
  );
}

interface SwitchProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  disabled?: boolean;
}

export function Switch({ checked, onChange, label, disabled = false }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={`switch ${checked ? "is-on" : ""}`}
      onClick={() => onChange(!checked)}
    >
      <span className="switch-thumb" />
    </button>
  );
}

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
}

interface SegmentedProps<T extends string> {
  value: T;
  options: SegmentedOption<T>[];
  onChange: (next: T) => void;
  label: string;
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: SegmentedProps<T>) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);

  function focusIndex(index: number) {
    const bounded = (index + options.length) % options.length;
    onChange(options[bounded].value);
    refs.current[bounded]?.focus();
  }

  function onKeyDown(event: KeyboardEvent, index: number) {
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      event.preventDefault();
      focusIndex(index + 1);
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      event.preventDefault();
      focusIndex(index - 1);
    }
  }

  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((option, index) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            ref={(node) => {
              refs.current[index] = node;
            }}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            className={`segmented-item ${selected ? "is-selected" : ""}`}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => onKeyDown(event, index)}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

interface SettingRowProps {
  title: string;
  description?: string;
  children: ReactNode;
  stack?: boolean;
}

export function SettingRow({ title, description, children, stack = false }: SettingRowProps) {
  return (
    <div className={`setting-row ${stack ? "is-stacked" : ""}`}>
      <div className="setting-copy">
        <div className="setting-title">{title}</div>
        {description && <p className="setting-desc">{description}</p>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}

interface ShortcutRecorderProps {
  value: string;
  onChange: (accelerator: string) => void;
  onRecordingChange?: (recording: boolean) => void;
}

export function ShortcutRecorder({
  value,
  onChange,
  onRecordingChange,
}: ShortcutRecorderProps) {
  const [recording, setRecording] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (recording) buttonRef.current?.focus();
    onRecordingChange?.(recording);
  }, [recording, onRecordingChange]);

  // Leaving Settings mid-recording must not leave the global shortcut suspended.
  useEffect(
    () => () => {
      onRecordingChange?.(false);
    },
    [onRecordingChange],
  );

  function handleKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (!recording) return;
    event.preventDefault();
    event.stopPropagation();

    if (event.key === "Escape") {
      setRecording(false);
      setMessage(null);
      return;
    }
    if (isPlatformModifier(event.nativeEvent)) {
      setMessage("再按一个字母、数字或功能键。");
      return;
    }

    const accelerator = acceleratorFromEvent(event.nativeEvent);
    if (!accelerator) {
      setMessage("请用 ⌘/Ctrl 或 Alt，加上字母、数字、F 键或方向键。");
      return;
    }

    onChange(accelerator);
    setRecording(false);
    setMessage(null);
  }

  return (
    <div className="shortcut-field">
      <button
        ref={buttonRef}
        type="button"
        className={`shortcut-recorder ${recording ? "is-recording" : ""}`}
        aria-label={`全局快捷键，当前为 ${shortcutText(value)}。点击录制新组合。`}
        onClick={() => {
          setRecording(true);
          setMessage(null);
        }}
        onKeyDown={handleKeyDown}
        onBlur={() => {
          if (recording) {
            setRecording(false);
            setMessage(null);
          }
        }}
      >
        {recording ? (
          <span className="recording-label">
            <span className="record-dot" />
            请按下快捷键…
          </span>
        ) : (
          <Keycaps accelerator={value} size="sm" />
        )}
      </button>
      <p className={`shortcut-hint ${message ? "is-error" : ""}`}>
        {message ?? "点击后按下新组合，Esc 取消。"}
      </p>
    </div>
  );
}
