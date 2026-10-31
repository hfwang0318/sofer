export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 MB";
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(0)} MB`;
  if (bytes >= 1_000) return `${(bytes / 1_000).toFixed(0)} KB`;
  return `${bytes} B`;
}

export function formatSpeed(bytesPerSecond: number): string {
  if (!bytesPerSecond) return "—";
  return `${(bytesPerSecond / 1_000_000).toFixed(1)} MB/s`;
}

export function formatEta(seconds: number | null): string {
  if (seconds == null) return "";
  if (seconds <= 0) return "almost done";
  if (seconds < 60) return `${Math.ceil(seconds)}s left`;
  const minutes = Math.floor(seconds / 60);
  const rest = Math.round(seconds % 60);
  return `${minutes}m ${rest.toString().padStart(2, "0")}s left`;
}

export function formatPlatform(platform: string): string {
  return platform
    .replace("macos-aarch64", "macOS · Apple Silicon")
    .replace("macos-x86_64", "macOS · Intel")
    .replace("windows-x86_64", "Windows · x64")
    .replace("windows-aarch64", "Windows · ARM64");
}
