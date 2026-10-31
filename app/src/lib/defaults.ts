import type {
  HistoryEntry,
  OcrResult,
  Settings,
} from "./types";

/** Must stay in sync with `Settings::default()` in src-tauri/src/settings.rs. */
export const DEFAULT_PROMPT =
  "OCR: preserve the original layout, spacing and line breaks exactly.";

export const DEFAULT_SETTINGS: Settings = {
  shortcut: "CommandOrControl+Shift+2",
  autoCopy: true,
  historyLimit: 20,
  downloadProxy: "",
  modelBase: "",
  engine: "auto",
  llamaUrl: "http://127.0.0.1:8099",
  prompt: DEFAULT_PROMPT,
  maxTokens: 3072,
  tessLang: "eng",
  psm: 6,
  tile: true,
  geometry: true,
  joinWrap: false,
  keepGutter: false,
  format: "text",
  language: "",
  jobs: 3,
};

export const MODEL_TOTAL_BYTES = 1_434_837_056;
export const MODEL_NAME = "GLM-OCR Q8_0";
/** Keep in sync with `engine::MODEL_REVISION` in src-tauri/src/engine.rs. */
export const MODEL_REVISION = "65a42de1148dbed2297e922b5dbc7d9b70c36578";
export const HF_MIRROR_BASE =
  `https://hf-mirror.com/ggml-org/GLM-OCR-GGUF/resolve/${MODEL_REVISION}`;
export const MODEL_LICENSE = "MIT";
export const SETUP_DISMISSED_KEY = "ocr4code.setup.dismissed";
export const SETTINGS_KEY = "ocr4code.settings.v1";
export const HISTORY_KEY = "ocr4code.history.v1";

const DEMO_TEXT = `async fn load_user(pool: &PgPool, id: UserId) -> Result<User> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, created_at FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;

    Ok(user)
}`;

const DEMO_THUMB = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" width="520" height="300" viewBox="0 0 520 300">
    <rect width="520" height="300" fill="#0b0c10"/>
    <rect x="0" y="0" width="520" height="26" fill="#14151a"/>
    <circle cx="14" cy="13" r="4" fill="#3a3d46"/><circle cx="28" cy="13" r="4" fill="#3a3d46"/><circle cx="42" cy="13" r="4" fill="#3a3d46"/>
    <g font-family="SFMono-Regular, Menlo, monospace" font-size="12">
      <text x="22" y="58" fill="#565a66">1</text><text x="48" y="58" fill="#cbf56c">async</text><text x="94" y="58" fill="#e6e8ec">fn</text><text x="116" y="58" fill="#8fb8ff">load_user</text><text x="184" y="58" fill="#e6e8ec">(</text>
      <text x="22" y="80" fill="#565a66">2</text><text x="48" y="80" fill="#e6e8ec">let</text><text x="72" y="80" fill="#e6e8ec">user =</text><text x="128" y="80" fill="#cbf56c">sqlx</text><text x="158" y="80" fill="#e6e8ec">::query_as</text>
      <text x="22" y="102" fill="#565a66">3</text><text x="76" y="102" fill="#8fb8ff">"SELECT id, email, created_at"</text>
      <text x="22" y="124" fill="#565a66">4</text><text x="76" y="124" fill="#8fb8ff">"FROM users WHERE id = $1"</text>
      <text x="22" y="146" fill="#565a66">5</text><text x="48" y="146" fill="#e6e8ec">)</text>
      <text x="22" y="168" fill="#565a66">6</text><text x="48" y="168" fill="#e6e8ec">.bind(id)</text>
      <text x="22" y="190" fill="#565a66">7</text><text x="48" y="190" fill="#e6e8ec">.await?;</text>
      <text x="22" y="212" fill="#565a66">8</text>
      <text x="22" y="234" fill="#565a66">9</text><text x="48" y="234" fill="#e6e8ec">Ok(user)</text>
    </g>
    <rect x="58" y="50" width="412" height="194" fill="none" stroke="#cbf56c" stroke-opacity=".55" stroke-width="1"/>
    <g fill="#cbf56c"><rect x="56" y="48" width="12" height="2"/><rect x="56" y="48" width="2" height="12"/><rect x="470" y="244" width="12" height="2"/><rect x="480" y="234" width="2" height="12"/></g>
  </svg>`,
)}`;

function historyEntry(
  offsetMs: number,
  text: string,
  engine: string,
  language: string,
  ms: number,
): HistoryEntry {
  return {
    id: `demo-${offsetMs}`,
    text,
    engine,
    ms,
    width: 1284,
    height: 742,
    language,
    format: "text",
    chars: text.length,
    lines: text.split("\n").length,
    capturedAt: Date.now() - offsetMs,
  };
}

export const DEMO_RESULT: OcrResult = {
  id: "demo-result",
  text: DEMO_TEXT,
  engine: "llamacpp(http://127.0.0.1:8099)",
  ms: 864,
  width: 1284,
  height: 742,
  language: "rust",
  format: "text",
  chars: DEMO_TEXT.length,
  lines: DEMO_TEXT.split("\n").length,
  thumbnail: DEMO_THUMB,
  capturedAt: Date.now() - 42_000,
  copied: true,
};

export const DEMO_HISTORY: HistoryEntry[] = [
  historyEntry(42_000, DEMO_TEXT, "llamacpp(http://127.0.0.1:8099)", "rust", 864),
  historyEntry(
    8 * 60_000,
    `$ cargo test --workspace
   Compiling ocr4code v0.1.2
    Finished test [unoptimized + debuginfo] target(s)
     Running unittests src/lib.rs

running 34 tests
test geom::tests::band_projection ... ok`,
    "llamacpp(http://127.0.0.1:8099)",
    "bash",
    1_240,
  ),
  historyEntry(
    26 * 60_000,
    `- let token = exchange(refresh_token).await?;
+ let token = exchange(refresh_token, device_id).await?;
+ metrics::refresh("web").increment(1);`,
    "tesseract(-l eng, --psm 6)",
    "diff",
    486,
  ),
];
