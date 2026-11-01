import { useEffect, useState, type ReactNode } from "react";

import { Icon } from "./Icon";
import { Segmented, Switch } from "./Controls";
import {
  remoteCancel,
  remoteDeploy,
  remoteProbe,
  remoteStart,
  remoteStop,
} from "../lib/bridge";
import { formatBytes, formatEta, formatSpeed } from "../lib/format";
import type {
  RemoteProbe,
  RemoteSource,
  RemoteState,
  Settings,
  ToastTone,
} from "../lib/types";

interface RemotePanelProps {
  draft: Settings;
  remote: RemoteState;
  onUpdate: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
  onSave: (draft: Settings) => Promise<void>;
  onNotify: (tone: ToastTone, title: string, detail?: string) => void;
}

function Field({
  label,
  hint,
  error,
  full = false,
  children,
}: {
  label: string;
  hint?: string;
  error?: string | null;
  full?: boolean;
  children: ReactNode;
}) {
  return (
    <div className={`field ${full ? "field-full" : ""}`}>
      <div className="field-head">
        <span className="field-label">{label}</span>
      </div>
      {children}
      {error ? (
        <span className="field-error">{error}</span>
      ) : hint ? (
        <span className="field-hint">{hint}</span>
      ) : null}
    </div>
  );
}

function busy(phase: string): boolean {
  return ["probing", "deploying", "starting"].includes(phase);
}

/** The one place the remote phase turns into first-layer words. */
function phaseBadge(remote: RemoteState): { label: string; className: string } {
  switch (remote.phase) {
    case "online":
      return { label: "已连接", className: "badge-ok" };
    case "reconnecting":
      return { label: "重连中", className: "badge-warn" };
    case "failed":
      return { label: "失败", className: "badge-warn" };
    case "starting":
      return { label: "连接中", className: "" };
    case "probing":
      return { label: "检测中", className: "" };
    case "deploying":
      return { label: "部署中", className: "" };
    default:
      return {
        label: remote.deployed ? "已部署" : "未部署",
        className: "",
      };
  }
}

/** "（第 2 次，共 5 次）" while the watchdog is retrying. */
function attemptNote(remote: RemoteState): string {
  const attempt = remote.reconnectAttempt;
  const limit = remote.reconnectLimit;
  if (typeof attempt !== "number" || typeof limit !== "number") return "";
  return `（第 ${attempt} 次，共 ${limit} 次）`;
}

export function RemotePanel({
  draft,
  remote,
  onUpdate,
  onSave,
  onNotify,
}: RemotePanelProps) {
  const [probing, setProbing] = useState(false);
  const [probe, setProbe] = useState<RemoteProbe | null>(null);
  const [error, setError] = useState<string | null>(null);

  const working = probing || busy(remote.phase);
  const progress = remote.progress;

  // Probe results describe one target; do not keep showing them after the
  // host, key or directory changes.
  useEffect(() => {
    setProbe(null);
    setError(null);
  }, [
    draft.remoteEnabled,
    draft.remoteHost,
    draft.remoteUser,
    draft.remoteSshPort,
    draft.remoteKey,
    draft.remoteDir,
    draft.remoteServerPort,
  ]);

  const hostError =
    draft.remoteEnabled && !draft.remoteHost.trim()
      ? "填写服务器地址，例如 192.168.1.20。"
      : null;
  const userError =
    draft.remoteEnabled && !draft.remoteUser.trim() ? "填写 SSH 用户名。" : null;
  const portError =
    draft.remoteSshPort < 1 || draft.remoteSshPort > 65535
      ? "SSH 端口应在 1–65535 之间。"
      : null;
  const serverPortError =
    draft.remoteServerPort < 1 || draft.remoteServerPort > 65535
      ? "服务端口应在 1–65535 之间。"
      : null;
  const localPortError =
    draft.remoteLocalPort !== 0 &&
    (draft.remoteLocalPort < 1 || draft.remoteLocalPort > 65535)
      ? "隧道端口应为 0（自动）或 1–65535。"
      : null;

  async function saveFirst(): Promise<boolean> {
    setError(null);
    try {
      await onSave(draft);
      return true;
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setError(message);
      onNotify("error", "设置没有保存", message);
      return false;
    }
  }

  async function handleProbe() {
    if (working) return;
    if (!draft.remoteEnabled) {
      onNotify("info", "先启用远程 OCR 引擎", "打开上面的开关并保存后再检测。");
      return;
    }
    if (!(await saveFirst())) return;
    setProbing(true);
    try {
      const result = await remoteProbe();
      setProbe(result);
      onNotify(
        "ok",
        "服务器可用",
        `${result.os} · ${result.arch} · ${result.dir}`,
      );
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setError(message);
      onNotify("error", "连不上服务器", message);
    } finally {
      setProbing(false);
    }
  }

  async function handleDeploy(source: RemoteSource) {
    if (working) return;
    if (!(await saveFirst())) return;
    try {
      await remoteDeploy(source);
      // The old probe describes the pre-deploy state; keeping it would make
      // the actions show "重新部署" even after the model has been installed.
      setProbe(null);
      setError(null);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setError(message);
      onNotify("error", "无法开始部署", message);
    }
  }

  async function handleStart() {
    if (working) return;
    if (!(await saveFirst())) return;
    try {
      await remoteStart();
      setError(null);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setError(message);
      onNotify("error", "无法启动远程 OCR 引擎", message);
    }
  }

  async function handleStop() {
    if (working) return;
    try {
      await remoteStop();
      setError(null);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setError(message);
      onNotify("error", "无法停止远程 OCR 引擎", message);
    }
  }

  async function handleCancel() {
    try {
      await remoteCancel();
    } catch (cause) {
      onNotify(
        "error",
        "无法取消部署",
        cause instanceof Error ? cause.message : String(cause),
      );
    }
  }

  const percent =
    progress && progress.total > 0
      ? Math.min(100, (progress.downloaded / progress.total) * 100)
      : 0;
  const missingModel =
    Boolean(probe) && !remote.deployed && !probe?.modelInstalled;
  const modelReady = Boolean(probe?.modelInstalled && probe?.modelVerified);
  // A deployed server does not need another "deploy": the deploy action would
  // only probe, reuse the verified weights and start the service. Only ask for
  // a deploy when something is actually missing or unverified.
  const needsDeploy =
    remote.phase === "deploying" ||
    !remote.deployed ||
    (probe !== null && !modelReady);
  const canConnect =
    remote.deployed &&
    !needsDeploy &&
    remote.phase !== "online" &&
    remote.phase !== "reconnecting";
  const badge = phaseBadge(remote);

  return (
    <details className="settings-disclosure" open={draft.remoteEnabled}>
      <summary>
        部署到服务器（可选）
        {draft.remoteEnabled && (
          <span className={`badge ${badge.className}`}>{badge.label}</span>
        )}
      </summary>
      <div className="disclosure-body">
        <p className="field-hint">
          把 OCR 模型放到一台 Linux 服务器上，本机通过 SSH 隧道使用它。适合本机内存紧张时；
          截图会发到这台服务器，不再启动本机模型。
        </p>
        {draft.engine === "tesseract" && (
          <p className="field-hint">
            当前引擎是 tesseract，远程部署不会生效；在高级里换成「自动」或
            「llama.cpp」后再使用。
          </p>
        )}

        <div className="field field-full">
          <div className="field-head">
            <span className="field-label">启用远程 OCR 引擎</span>
            <Switch
              checked={draft.remoteEnabled}
              onChange={(remoteEnabled) => onUpdate("remoteEnabled", remoteEnabled)}
              label="启用远程 OCR 引擎"
            />
          </div>
          <span className="field-hint">
            需要服务器已配置 SSH 密钥或 ssh-agent；密码登录不支持。服务只监听服务器本机，
            由隧道转发。
          </span>
        </div>

        {draft.remoteEnabled && (
          <>
            <div className="field-grid">
              <Field label="服务器地址" error={hostError} hint="主机名或 IP，例如 build-box。">
                <input
                  className="input mono"
                  value={draft.remoteHost}
                  spellCheck={false}
                  autoComplete="off"
                  aria-label="远程服务器地址"
                  aria-invalid={Boolean(hostError)}
                  onChange={(event) => onUpdate("remoteHost", event.target.value)}
                />
              </Field>
              <Field label="SSH 用户名" error={userError} hint="使用密钥或 ssh-agent 登录。">
                <input
                  className="input mono"
                  value={draft.remoteUser}
                  spellCheck={false}
                  autoComplete="username"
                  aria-label="SSH 用户名"
                  aria-invalid={Boolean(userError)}
                  onChange={(event) => onUpdate("remoteUser", event.target.value)}
                />
              </Field>
              <Field label="SSH 端口" error={portError}>
                <input
                  className="input mono"
                  type="number"
                  min={1}
                  max={65535}
                  value={draft.remoteSshPort}
                  aria-label="SSH 端口"
                  aria-invalid={Boolean(portError)}
                  onChange={(event) =>
                    onUpdate("remoteSshPort", Number(event.target.value))
                  }
                />
              </Field>
              <Field
                label="私钥路径（可选）"
                hint="留空则使用 ssh-agent 或 ~/.ssh/config。"
              >
                <input
                  className="input mono"
                  value={draft.remoteKey}
                  spellCheck={false}
                  autoComplete="off"
                  aria-label="私钥路径"
                  onChange={(event) => onUpdate("remoteKey", event.target.value)}
                />
              </Field>
              <Field
                label="远程目录"
                hint="模型、运行时和日志放这里；支持 ~ 开头。"
                full
              >
                <input
                  className="input mono"
                  value={draft.remoteDir}
                  spellCheck={false}
                  autoComplete="off"
                  aria-label="远程安装目录"
                  onChange={(event) => onUpdate("remoteDir", event.target.value)}
                />
              </Field>
              <Field label="服务端口" error={serverPortError} hint="服务器回环地址上的端口。">
                <input
                  className="input mono"
                  type="number"
                  min={1}
                  max={65535}
                  value={draft.remoteServerPort}
                  aria-label="远程服务端口"
                  aria-invalid={Boolean(serverPortError)}
                  onChange={(event) =>
                    onUpdate("remoteServerPort", Number(event.target.value))
                  }
                />
              </Field>
              <Field
                label="隧道本地端口"
                error={localPortError}
                hint="0 表示自动选择空闲端口。"
              >
                <input
                  className="input mono"
                  type="number"
                  min={0}
                  max={65535}
                  value={draft.remoteLocalPort}
                  aria-label="隧道本地端口"
                  aria-invalid={Boolean(localPortError)}
                  onChange={(event) =>
                    onUpdate("remoteLocalPort", Number(event.target.value))
                  }
                />
              </Field>
              <Field
                label="加速方式"
                hint="服务器已有运行时则直接复用；没有时按这里下载。"
                full
              >
                <Segmented
                  label="加速方式"
                  value={draft.remoteAccel}
                  onChange={(next) => onUpdate("remoteAccel", next)}
                  options={[
                    { value: "auto", label: "自动（CPU）" },
                    { value: "cpu", label: "CPU" },
                    { value: "vulkan", label: "Vulkan" },
                    { value: "cuda", label: "CUDA" },
                  ]}
                />
              </Field>
              {draft.remoteAccel !== "cpu" && draft.remoteAccel !== "auto" && (
                <Field
                  label={`GPU 层数 — ${draft.remoteGpuLayers}`}
                  hint="99 表示尽量全部放 GPU；显存不够时调小。"
                  full
                >
                  <div className="range-row">
                    <input
                      type="range"
                      min={0}
                      max={99}
                      step={1}
                      value={draft.remoteGpuLayers}
                      aria-label="GPU 层数"
                      onChange={(event) =>
                        onUpdate("remoteGpuLayers", Number(event.target.value))
                      }
                    />
                    <span className="range-value">{draft.remoteGpuLayers}</span>
                  </div>
                </Field>
              )}
              <Field
                label="模型来源"
                hint="自动：服务器已有就复用，否则在服务器下载。"
                full
              >
                <Segmented
                  label="模型来源"
                  value={draft.remoteSource}
                  onChange={(next) => onUpdate("remoteSource", next)}
                  options={[
                    { value: "auto", label: "自动" },
                    { value: "download", label: "服务器下载" },
                    { value: "upload", label: "上传本机模型" },
                  ]}
                />
              </Field>
              <div className="field field-full">
                <div className="field-head">
                  <span className="field-label">启动时自动连接</span>
                  <Switch
                    checked={draft.remoteAutoStart}
                    onChange={(remoteAutoStart) =>
                      onUpdate("remoteAutoStart", remoteAutoStart)
                    }
                    label="启动时自动连接"
                  />
                </div>
                <span className="field-hint">
                  应用启动后在后台建立隧道；失败不会打断截图。
                </span>
              </div>
            </div>

            {remote.phase === "reconnecting" && (
              <p className="field-hint" role="status" aria-live="polite">
                <Icon name="refresh" size={13} className="is-spinning" />
                连接断开了，正在自动重连{attemptNote(remote)}。
              </p>
            )}

            <div className="runtime-actions remote-actions">
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={() => void handleProbe()}
                // A probe cancels the watchdog; do not let it interrupt a
                // reconnect that is already healing the tunnel.
                disabled={working || remote.phase === "reconnecting"}
              >
                {probing || remote.phase === "probing" ? "检测中…" : "检测服务器"}
              </button>
              {needsDeploy && (
                <button
                  type="button"
                  className="btn btn-primary btn-sm"
                  onClick={() => void handleDeploy(draft.remoteSource)}
                  disabled={working}
                >
                  <Icon name="arrow-right" size={14} />
                  {working && remote.phase === "deploying"
                    ? "正在部署…"
                    : remote.deployed
                      ? "重新部署"
                      : "一键部署"}
                </button>
              )}
              {canConnect && (
                <button
                  type="button"
                  className="btn btn-primary btn-sm"
                  onClick={() => void handleStart()}
                  disabled={working}
                >
                  <Icon name="power" size={14} />
                  {remote.phase === "starting"
                    ? "连接中…"
                    : remote.phase === "failed"
                      ? "重连"
                      : "连接"}
                </button>
              )}
              {((remote.deployed && remote.phase === "online") ||
                remote.phase === "reconnecting") && (
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => void handleStop()}
                    disabled={working}
                  >
                    {remote.phase === "reconnecting"
                      ? "停止重连"
                      : "停止远程服务"}
                  </button>
                )}
              {remote.phase === "deploying" && (
                <button
                  type="button"
                  className="link-btn"
                  onClick={() => void handleCancel()}
                >
                  取消
                </button>
              )}
            </div>

            {remote.deployed && !needsDeploy && remote.phase !== "online" && (
              <p className="field-hint">
                模型已在服务器上，连接会复用它，不会重新下载。
              </p>
            )}

            {progress && remote.phase !== "online" && (
              <div className="runtime-download" role="status" aria-live="polite">
                <div className="runtime-download-head">
                  <span>{progress.message ?? progress.phase}</span>
                  <span className="mono">
                    {progress.phase === "downloading" || progress.phase === "uploading"
                      ? `${Math.round(percent)}% · ${formatSpeed(progress.bytesPerSec)}${
                          progress.etaSecs != null
                            ? ` · ${formatEta(progress.etaSecs)}`
                            : ""
                        }`
                      : "…"}
                  </span>
                </div>
                <div className="runtime-bar">
                  <span style={{ width: `${percent}%` }} />
                </div>
              </div>
            )}

            {probe && (
              <div className="runtime-grid remote-probe">
                <div>
                  <span className="runtime-label">服务器</span>
                  <span className="runtime-value">
                    {probe.os || "未知"} · {probe.arch || "未知"}
                  </span>
                </div>
                <div>
                  <span className="runtime-label">安装目录</span>
                  <span className="runtime-value mono">{probe.dir}</span>
                </div>
                <div>
                  <span className="runtime-label">OCR 模型</span>
                  <span className="runtime-value">
                    {probe.modelInstalled
                      ? `${probe.modelVerified ? "已校验" : "未校验"} · 已就绪`
                      : "还没有，部署时下载或上传"}
                  </span>
                </div>
                <div>
                  <span className="runtime-label">运行时</span>
                  <span className="runtime-value">
                    {probe.runtime ? "已安装" : "部署时自动下载"}
                  </span>
                </div>
                <div>
                  <span className="runtime-label">可用空间</span>
                  <span className="runtime-value">{formatBytes(probe.diskFree)}</span>
                </div>
                <div>
                  <span className="runtime-label">服务器工具</span>
                  <span className="runtime-value">
                    {[
                      probe.curl && "curl",
                      probe.wget && "wget",
                      probe.tar && "tar",
                      probe.sha256 || "缺少 sha256",
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </span>
                </div>
              </div>
            )}

            {missingModel && (
              <div className="remote-source-choice">
                <p>
                  服务器还没有 OCR 模型。选择一种来源；1.0–1.4 GB，只需一次。
                </p>
                <div>
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => void handleDeploy("download")}
                    disabled={working}
                  >
                    在服务器下载
                  </button>
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => void handleDeploy("upload")}
                    disabled={working}
                  >
                    上传本机模型
                  </button>
                </div>
                <span className="field-hint">
                  上传需要本机已下载同一精度的模型；服务器下载需要服务器能访问
                  Hugging Face。
                </span>
              </div>
            )}

            {(error || remote.error) && (
              <p className="runtime-error">
                <Icon name="alert" size={13} />
                {error ?? remote.error}
              </p>
            )}
          </>
        )}
      </div>
    </details>
  );
}
