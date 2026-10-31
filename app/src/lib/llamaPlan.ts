/** Must stay in sync with `PROMPT_TOKEN_BUDGET` / `CONTEXT_MARGIN` in
 * `src-tauri/src/engine.rs`. The UI uses this only for the live hint; the Rust
 * side is the source of truth for the actual llama-server arguments. */
export const PROMPT_TOKEN_BUDGET = 6656;
export const CONTEXT_MARGIN = 1280;

export function llamaSlots(jobs: number): number {
  return Math.min(3, Math.max(1, Math.trunc(jobs)));
}

export function perSlotCtx(maxTokens: number): number {
  const raw = PROMPT_TOKEN_BUDGET + maxTokens + CONTEXT_MARGIN;
  return Math.max(8192, Math.ceil(raw / 256) * 256);
}

export function totalCtx(jobs: number, maxTokens: number): number {
  return llamaSlots(jobs) * perSlotCtx(maxTokens);
}
