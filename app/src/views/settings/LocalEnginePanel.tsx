import { useEffect, useState } from "react";

import { Icon } from "../../components/Icon";
import { formatBytes, formatEta, formatSpeed } from "../../lib/format";
import type {
  DownloadProgress,
  EngineBusy,
  EngineStatus,
} from "../../lib/types";

interface LocalEnginePanelProps {
  status: EngineStatus;
  download: DownloadProgress | null;
  engineBusy: EngineBusy;
  onSetup: () => void;
  onDownloadModel: () => void;
  onStartEngine: () => void;
  onStopEngine: () => void;
  onRemoveModel: () => void;
  onCancelSetup: () => void;
}

/** Managed llama.cpp server: runtime, model pack and download progress. */
export function LocalEnginePanel({
  status,
  download,
  engineBusy,
  onSetup,
  onDownloadModel,
  onStartEngine,
  onStopEngine,
  onRemoveModel,
  onCancelSetup,
}: LocalEnginePanelProps) {
  const [confirmRemove, setConfirmRemove] = useState(false);
  const { runtime, model, server } = status;

  useEffect(() => {
    if (!confirmRemove) return;
    const timer = window.setTimeout(() => setConfirmRemove(false), 5000);
    return () => window.clearTimeout(timer);
  }, [confirmRemove]);
  const busyPhase = download?.phase;
  const downloading = Boolean(
    busyPhase && ["downloading", "verifying", "starting"].includes(busyPhase),
  );
  const percent =
    download && download.total > 0
      ? Math.min(100, (download.downloaded / download.total) * 100)
      : 0;
  const managed = server.managed;
  const externalOnline = !managed && server.state === "online";
  const busy = engineBusy !== null;

  return (
    <div className="runtime-panel">
      <div className="runtime-head">
        <div className="runtime-heading">
          <span className={`status-dot tone-${status.llama ? "ok" : model.installed ? "warn" : "danger"}`} />
          <strong>本地 OCR 引擎</strong>
          <span className="badge">
            {runtime.bundled ? "内置运行时" : runtime.system ? "系统运行时" : "运行时缺失"}
          </span>
          {model.installed && (
            <span className={`badge ${model.verified ? "badge-ok" : "badge-warn"}`}>
              {model.verified ? "已校验" : "未校验"}
            </span>
          )}
        </div>
        <div className="runtime-actions">
          {downloading ? (
            <span className="badge">
              {busyPhase === "verifying"
                ? "校验中"
                : busyPhase === "starting"
                  ? "加载中"
                  : "下载中"}
            </span>
          ) : externalOnline ? (
            <span className="badge badge-ok">外部服务</span>
          ) : !model.installed ? (
            <button type="button" className="btn btn-primary btn-sm" onClick={onDownloadModel} disabled={busy}>
              <Icon name="arrow-right" size={14} />
              下载模型
            </button>
          ) : server.state === "starting" ? (
            <button type="button" className="btn btn-secondary btn-sm" disabled>
              <span className="btn-spinner btn-spinner-muted" />
              正在启动…
            </button>
          ) : managed && server.state === "online" ? (
            <button type="button" className="btn btn-secondary btn-sm" onClick={onStopEngine} disabled={busy}>
              {engineBusy === "stop" ? "正在停止…" : "停止"}
            </button>
          ) : (
            <button type="button" className="btn btn-secondary btn-sm" onClick={onStartEngine} disabled={busy}>
              <Icon name="power" size={14} />
              {engineBusy === "start" ? "正在启动…" : "启动"}
            </button>
          )}
        </div>
      </div>

      <div className="runtime-grid">
        <div>
          <span className="runtime-label">运行时</span>
          <span className="runtime-value">
            {runtime.version ?? (runtime.bundled || runtime.system ? "llama.cpp" : "未找到")}
          </span>
        </div>
        <div>
          <span className="runtime-label">模型</span>
          <span className="runtime-value">
            {externalOnline && !model.installed
              ? "未下载 · 当前使用外部服务"
              : `${status.setup.modelName} · ${
                  model.installed
                    ? `${formatBytes(model.bytes)} · ${model.verified ? "已校验" : "未校验"}`
                    : `${formatBytes(model.total)} 待下载`
                }`}
          </span>
        </div>
        <div>
          <span className="runtime-label">服务</span>
          <span className={`runtime-value tone-${server.state === "online" ? "ok" : server.state === "failed" ? "danger" : "muted"}`}>
            {server.state === "online"
              ? `${managed ? "本地" : "外部"} · ${server.url ?? status.llamaUrl}`
              : server.state === "starting"
                ? "正在启动…"
                : server.state === "failed"
                  ? "启动失败"
                  : "已停止"}
          </span>
        </div>
      </div>

      {downloading && download && (
        <div className="runtime-download">
          <div className="runtime-download-head">
            <span>
              {download.phase === "verifying"
                ? "正在校验文件…"
                : download.phase === "starting"
                  ? "正在加载模型…"
                  : download.file ?? "正在下载模型"}
            </span>
            <span className="mono">
              {download.phase === "downloading"
                ? `${Math.round(percent)}% · ${formatSpeed(download.bytesPerSec)}${
                    download.etaSecs != null ? ` · ${formatEta(download.etaSecs)}` : ""
                  }`
                : "…"}
            </span>
          </div>
          <div className="runtime-bar">
            <span style={{ width: `${download.phase === "starting" ? 100 : percent}%` }} />
          </div>
          {download.phase === "downloading" ? (
            <button type="button" className="link-btn" onClick={onCancelSetup}>
              取消下载
            </button>
          ) : (
            <span className="runtime-foot-note">完成后会自动继续</span>
          )}
        </div>
      )}

      {download?.phase === "error" && (
        <p className="runtime-error">
          <Icon name="alert" size={13} />
          {download.message ?? "没有完成。"}
          <button type="button" className="link-btn" onClick={onSetup}>
            重试
          </button>
        </p>
      )}

      {server.state === "failed" && server.error && (
        <p className="runtime-error">
          <Icon name="alert" size={13} />
          {server.error.split("\n")[0]}
        </p>
      )}

      <div className="runtime-foot">
        <button type="button" className="link-btn" onClick={onSetup}>
          {model.installed ? "重新设置" : "打开设置向导"}
        </button>
        {model.installed && (
          <button
            type="button"
            className="link-btn link-btn-danger"
            onClick={() => setConfirmRemove(true)}
            disabled={busy}
          >
            移除模型
          </button>
        )}
        <span className="runtime-foot-note">
          {externalOnline
            ? "当前使用外部服务"
            : model.installed
              ? "本地处理"
              : `下载 ${formatBytes(model.total)} · 支持续传`}
        </span>
      </div>

      {confirmRemove && (
        <div className="runtime-confirm">
          <span>移除当前模型？下次截图前需要重新下载。</span>
          <div>
            <button
              type="button"
              className="btn btn-danger btn-sm"
              onClick={() => {
                setConfirmRemove(false);
                onRemoveModel();
              }}
              disabled={busy}
            >
              移除
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => setConfirmRemove(false)}
            >
              取消
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
