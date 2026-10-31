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
  if (seconds <= 0) return "即将完成";
  if (seconds < 60) return `剩余 ${Math.ceil(seconds)} 秒`;
  const minutes = Math.floor(seconds / 60);
  const rest = Math.round(seconds % 60);
  return `剩余 ${minutes} 分 ${rest.toString().padStart(2, "0")} 秒`;
}

export function formatPlatform(platform: string): string {
  return platform
    .replace("macos-aarch64", "macOS · Apple 芯片")
    .replace("macos-x86_64", "macOS · Intel")
    .replace("windows-x86_64", "Windows · x64")
    .replace("windows-aarch64", "Windows · ARM64");
}
