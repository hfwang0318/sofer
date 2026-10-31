/**
 * Browser-preview only. The real window is transparent and NSVisualEffectView /
 * Acrylic supplies the desktop blur; this gives screenshots and `pnpm dev` the
 * same depth without shipping a fake wallpaper in the Tauri build.
 */
export function PreviewDesktop() {
  return (
    <div className="preview-desktop" aria-hidden="true">
      <div className="preview-wash wash-one" />
      <div className="preview-wash wash-two" />
      <div className="preview-editor">
        <div className="preview-editor-bar">
          <span />
          <span />
          <span />
          <em>session.ts — workspace</em>
        </div>
        <pre>
          <span className="pe-dim">1</span>{"  "}
          <span className="pe-purple">export async function</span>{" "}
          <span className="pe-blue">refreshSession</span>() {"{"}
          {"\n"}
          <span className="pe-dim">2</span>{"    "}
          <span className="pe-purple">const</span> token ={" "}
          <span className="pe-purple">await</span> exchange(refreshToken);{"\n"}
          <span className="pe-dim">3</span>{"    "}
          <span className="pe-purple">await</span> cache.set(
          <span className="pe-green">`session:${"{token.id}"}`</span>, token);{"\n"}
          <span className="pe-dim">4</span>{"    "}
          <span className="pe-purple">return</span> {"{"} ok:{" "}
          <span className="pe-orange">true</span>, token {"}"};{"\n"}
          <span className="pe-dim">5</span>{"}"}
        </pre>
      </div>
      <div className="preview-dock">
        <span />
        <span />
        <span />
        <span />
        <span />
      </div>
    </div>
  );
}
