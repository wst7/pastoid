/**
 * 设置窗口应用
 *
 * 版式：单列分组卡片（外观 / 剪贴板 / 快捷键）+ 常驻页脚。
 * 不分页签——只有六项设置，标签页反而把「是否最新」这种状态藏了起来。
 */
import { useEffect, useRef, useState } from "react";
import packageInfo from "../../package.json" with { type: "json" };
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { check, Update } from "@tauri-apps/plugin-updater";
import { applyTheme, paint, watchSystemTheme, type ThemePref } from "@/lib/theme";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { open as openDialog, message } from "@tauri-apps/plugin-dialog";
import { Switch, Button } from "@heroui/react";
import ShortcutRecordInput from "@/components/ShortcutRecordInput";
import { defaultShortcut } from "@/lib/platform";
import { FolderOpen, X } from "lucide-react";
import { Select } from "@/components/Select";

interface UpdateStatus {
  update: Update | null;
  current_version: string;
}

interface SettingsData {
  language: string;
  theme: string;
  autostart: boolean;
  max_items: number;
  shortcut: string;
}

interface StorageInfo {
  data_dir: string;
  default_dir: string;
  is_custom: boolean;
  first_run: boolean;
  total_bytes: number;
  image_count: number;
  item_count: number;
  /** 已选好、还没重启生效的目录 */
  pending_dir?: string | null;
}

const languages = [
  { id: "zh", name: "简体中文" },
  { id: "en", name: "English" },
];

const themes = (t: (key: string) => string) => [
  { id: "system", name: t("system") },
  { id: "light", name: t("light") },
  { id: "dark", name: t("dark") },
];

function formatBytes(bytes: number): string {
  if (!bytes) return "0 B";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[i]}`;
}

/** 一行设置：左标签、右控件 */
function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex min-h-[42px] items-center justify-between gap-3 border-b border-zinc-200/70 px-3 last:border-b-0 dark:border-white/10">
      <span className="text-[13px] text-foreground">{label}</span>
      <div className="flex items-center gap-1.5">{children}</div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mb-3">
      <h2 className="mb-1.5 px-1 text-[11px] font-semibold tracking-wide text-muted-foreground">
        {title}
      </h2>
      <div className="overflow-hidden rounded-xl bg-card shadow-sm ring-1 ring-black/5 dark:ring-white/10">
        {children}
      </div>
    </section>
  );
}

export default function SettingsApp() {
  const { t, i18n } = useTranslation();

  const [settings, setSettings] = useState<SettingsData>({
    language: "zh",
    theme: "system",
    autostart: false,
    max_items: 20,
    shortcut: defaultShortcut,
  });
  const [loadingSettings, setLoadingSettings] = useState(true);
  // 系统明暗变化时要知道现在是不是「跟随系统」档
  const themeRef = useRef<ThemePref>("system");
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  const [storageError, setStorageError] = useState<string | null>(null);
  const [pendingDataDir, setPendingDataDir] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);

  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [installed, setInstalled] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);

  const oldShortcutRef = useRef(settings.shortcut);

  useEffect(() => {
    loadSettings();
    loadStorage();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<string>("shortcut-error", (event) => {
      console.warn("Shortcut registration failed:", event.payload);
      setSettings((prev) => ({
        ...prev,
        shortcut: oldShortcutRef.current,
      }));
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // 系统外观变化（夜间自动变暗等）：只在「跟随系统」档响应
  useEffect(() => {
    return watchSystemTheme(() => {
      if (themeRef.current === "system") paint("system");
    });
  }, []);

  const loadSettings = async () => {
    try {
      const data = await invoke<SettingsData>("get_settings");
      setSettings({
        language: data.language || "zh",
        theme: data.theme || "light",
        autostart: data.autostart ?? false,
        max_items: data.max_items ?? 20,
        shortcut: data.shortcut || defaultShortcut,
      });
      themeRef.current = (data.theme as ThemePref) || "system";
      applyTheme(themeRef.current);
      if (data.language) {
        i18n.changeLanguage(data.language === "zh" ? "zh" : "en");
      }
    } catch (error) {
      console.error("Failed to load settings:", error);
    } finally {
      setLoadingSettings(false);
    }
  };

  const saveSettings = async (newSettings: SettingsData) => {
    try {
      await invoke("save_settings", { settings: newSettings });
      themeRef.current = newSettings.theme as ThemePref;
      applyTheme(themeRef.current);
    } catch (error) {
      console.error("Failed to save settings:", error);
    }
  };

  const loadStorage = async () => {
    try {
      setStorage(await invoke<StorageInfo>("get_storage_info"));
      setStorageError(null);
    } catch (error) {
      console.error("Failed to load storage info:", error);
      setStorageError(String(error));
    }
  };

  const handleLanguageChange = (language: string) => {
    const newSettings = { ...settings, language };
    setSettings(newSettings);
    i18n.changeLanguage(language === "zh" ? "zh" : "en");
    saveSettings(newSettings);
  };

  const handleThemeChange = (theme: string) => {
    const newSettings = { ...settings, theme };
    setSettings(newSettings);
    saveSettings(newSettings);
  };

  const handleAutostartChange = (isSelected: boolean) => {
    const newSettings = { ...settings, autostart: isSelected };
    setSettings(newSettings);
    saveSettings(newSettings);
  };

  const handleMaxItemsChange = async (value: string) => {
    const max_items = Math.max(1, Math.min(100, parseInt(value) || 20));
    const newSettings = { ...settings, max_items };
    setSettings(newSettings);
    await saveSettings(newSettings);
  };

  const handleChangeDataDir = async () => {
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: t("dataDirPickTitle"),
      });
      if (typeof picked !== "string") return;

      await invoke("set_data_dir", { path: picked });
      setPendingDataDir(true);
      await loadStorage();
    } catch (error) {
      console.error("Failed to set data dir:", error);
      await message(String(error), { title: "Pastoid", kind: "error" });
    }
  };

  const handleUseDefaultDir = async () => {
    try {
      await invoke("mark_first_run_done");
      await loadStorage();
    } catch (error) {
      console.error("Failed to mark first run:", error);
    }
  };

  const handleOpenDataDir = async () => {
    try {
      await invoke("open_data_dir");
    } catch (error) {
      console.error("Failed to open data dir:", error);
    }
  };

  /**
   * 录制期间全局热键是停掉的（否则按下的组合会先被全局监听抢走），
   * 所以这里每条退出路径都要把它恢复回去。
   */
  const handleShortcutRecordingChange = (recording: boolean) => {
    invoke("set_global_shortcut_enabled", { enabled: !recording }).catch((error) =>
      console.error("Failed to toggle shortcut:", error)
    );
  };

  const handleShortcutChange = async (shortcut: string) => {
    oldShortcutRef.current = settings.shortcut;

    // 录到的和当前一样：没有变更可保存，把刚才停掉的热键直接恢复
    if (shortcut === settings.shortcut) {
      await handleShortcutRecordingChange(false);
      return;
    }

    const newSettings = { ...settings, shortcut };
    setSettings(newSettings);
    await saveSettings(newSettings);
  };

  const checkForUpdate = async () => {
    setCheckingUpdate(true);
    try {
      const update = await check();
      setUpdateStatus({
        update: update || null,
        current_version: packageInfo.version,
      });
    } catch (error) {
      console.error("Failed to check update:", error);
    } finally {
      setCheckingUpdate(false);
    }
  };

  const handleDownloadAndInstall = async () => {
    if (!updateStatus?.update) return;

    setIsDownloading(true);
    try {
      let downloaded = 0;
      let contentLength = 0;
      await updateStatus.update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            contentLength = event.data.contentLength || 0;
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            if (contentLength > 0) {
              setDownloadProgress(Math.round((downloaded / contentLength) * 100));
            }
            break;
          case "Finished":
            setDownloadProgress(100);
            break;
        }
      });
      // 新版已经解包替换到磁盘，但当前进程跑的仍是旧代码，必须重启才生效
      setInstalled(true);
      await new Promise((r) => setTimeout(r, 800));
      await relaunch();
    } catch (error) {
      console.error("Download or install failed:", error);
      setUpdateError(String(error));
    } finally {
      setIsDownloading(false);
    }
  };

  const themeOptions = themes(t);

  const maxItems = settings.max_items || 20;
  const itemCount = storage?.item_count ?? 0;
  const usageRatio = Math.min(100, Math.round((itemCount / maxItems) * 100));

  const hasUpdate = Boolean(updateStatus?.update);
  const statusDot = updateError
    ? "bg-red-500"
    : !updateStatus
      ? "bg-zinc-400"
      : hasUpdate
        ? "bg-amber-500"
        : "bg-green-500";
  const statusLabel = updateError
    ? `${t("updateFailed")}（${updateError}）`
    : !updateStatus
      ? ""
      : hasUpdate
        ? t("updateAvailable", { version: updateStatus.update?.version ?? "" })
        : t("upToDate");

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      {storage?.first_run && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-sm">
          <div className="w-[440px] space-y-4 rounded-xl border border-border bg-card p-6 shadow-xl">
            <h2 className="text-base font-semibold">{t("firstRunTitle")}</h2>
            <p className="text-sm leading-relaxed text-muted-foreground">{t("firstRunHint")}</p>
            <p className="break-all rounded-lg bg-default px-3 py-2 font-mono text-xs text-muted-foreground">
              {storage.default_dir}
            </p>
            <div className="flex justify-end gap-2 pt-1">
              <Button variant="ghost" onPress={handleUseDefaultDir}>
                {t("firstRunDefault")}
              </Button>
              <Button variant="primary" onPress={handleChangeDataDir}>
                {t("firstRunPick")}
              </Button>
            </div>
          </div>
        </div>
      )}

      {aboutOpen && (
        <div
          className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-4 backdrop-blur-sm"
          onClick={() => setAboutOpen(false)}
        >
          <div
            className="w-[400px] space-y-4 rounded-2xl bg-card p-6 shadow-2xl ring-1 ring-black/5 dark:ring-white/10"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-start justify-between">
              <div className="flex items-center gap-3">
                <img src="/logo.svg" alt="Pastoid" className="h-12 w-12 rounded-[13px]" />
                <div>
                  <h2 className="text-lg font-semibold leading-tight">{t("appName")}</h2>
                  <p className="text-xs text-muted-foreground">v{packageInfo.version}</p>
                </div>
              </div>
              <button
                onClick={() => setAboutOpen(false)}
                aria-label={t("closePanel")}
                className="rounded-md p-1 text-muted-foreground hover:bg-default"
              >
                <X size={16} />
              </button>
            </div>

            <p className="text-sm leading-relaxed text-muted-foreground">{t("description")}</p>

            <div className="flex items-center justify-center gap-4 text-xs">
              <button
                onClick={() => openUrl("https://github.com/wst7/pastoid")}
                className="text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
              >
                {t("aboutSource")}
              </button>
              <button
                onClick={() => openUrl("https://github.com/wst7/pastoid/issues")}
                className="text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
              >
                {t("aboutFeedback")}
              </button>
            </div>

            <p className="text-center text-[11px] text-muted-foreground">
              © 2026 Pastoid. All rights reserved.
            </p>
          </div>
        </div>
      )}

      <header className="flex flex-none items-center gap-2.5 px-4 pb-2 pt-[38px]">
        <img src="/logo.svg" alt="" className="h-9 w-9 rounded-[9px]" />
        <div className="min-w-0">
          <h1 className="text-[15px] font-semibold leading-tight">{t("settings")}</h1>
          <p className="truncate text-[11.5px] text-muted-foreground">{t("settingsSubtitle")}</p>
        </div>
        <span className="ml-auto shrink-0 rounded-full bg-default px-2 py-0.5 text-[11px] text-muted-foreground">
          v{packageInfo.version}
        </span>
      </header>

      <main className="flex-1 overflow-y-auto px-4 pb-1">
        <Section title={t("sectionAppearance")}>
          <Row label={t("language")}>
            <div className="relative flex items-center">
              <Select
                value={settings.language}
                options={languages}
                onChange={handleLanguageChange}
                disabled={loadingSettings}
                minWidth={140}
              />
            </div>
          </Row>

          <Row label={t("theme")}>
            <div className="relative flex items-center">
              <Select
                value={settings.theme}
                options={themeOptions}
                onChange={handleThemeChange}
                disabled={loadingSettings}
                minWidth={120}
              />
            </div>
          </Row>

          <Row label={t("autoStart")}>
            <Switch
              isSelected={settings.autostart}
              onChange={handleAutostartChange}
              isDisabled={loadingSettings}
              style={
                {
                  "--color-accent": "var(--primary)",
                  "--accent-foreground": "#ffffff",
                } as React.CSSProperties
              }
            >
              <Switch.Control>
                <Switch.Thumb />
              </Switch.Control>
            </Switch>
          </Row>
        </Section>

        <Section title={t("sectionClipboard")}>
          <Row label={t("maxItems")}>
            <input
              type="number"
              min={1}
              max={100}
              value={String(settings.max_items)}
              onChange={(e) => handleMaxItemsChange(e.target.value)}
              disabled={loadingSettings}
              className="h-7 w-14 rounded-md border border-zinc-200 bg-transparent px-2 text-center text-[13px] tabular-nums focus:outline-none focus:ring-2 focus:ring-blue-500/40 dark:border-white/15"
            />
          </Row>

          <div className="border-t border-zinc-200/70 px-3 py-2.5 dark:border-white/10">
            <div className="flex items-center justify-between gap-2">
              <span className="text-[13px]">{t("dataDir")}</span>
              <div className="flex items-center gap-1">
                <Button
                  size="sm"
                  variant="ghost"
                  onPress={handleOpenDataDir}
                  isDisabled={loadingSettings}
                >
                  <FolderOpen size={14} />
                  {t("dataDirOpen")}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onPress={handleChangeDataDir}
                  isDisabled={loadingSettings}
                >
                  {t("dataDirChange")}
                </Button>
              </div>
            </div>

            <div className="mt-1 space-y-0.5">
              <p
                className="break-all font-mono text-[11px] leading-relaxed text-muted-foreground"
                title={storage?.data_dir ?? ""}
              >
                {storage?.data_dir ??
                  (storageError ? `${t("dataDirLoadFailed")}: ${storageError}` : t("loading"))}
              </p>
              {storage?.pending_dir && (
                <p
                  className="break-all font-mono text-[11px] leading-relaxed text-amber-600 dark:text-amber-400"
                  title={storage.pending_dir}
                >
                  {t("dataDirPending")}: {storage.pending_dir}
                </p>
              )}
            </div>

            <div className="mt-2 flex items-center gap-2">
              <div className="h-1 flex-1 overflow-hidden rounded-full bg-zinc-200/80 dark:bg-white/10">
                <div
                  className="h-full rounded-full bg-blue-500 transition-all"
                  style={{ width: `${usageRatio}%` }}
                />
              </div>
              <span className="shrink-0 text-[11px] text-muted-foreground">
                {t("storageUsage", {
                  items: itemCount,
                  max: maxItems,
                  size: formatBytes(storage?.total_bytes ?? 0),
                })}
                {" · "}
                {t("storageImages", { count: storage?.image_count ?? 0 })}
              </span>
            </div>

            {(pendingDataDir || Boolean(storage?.pending_dir)) && (
              <div className="mt-2 flex items-center justify-between gap-3 rounded-lg bg-amber-500/10 px-2.5 py-1.5">
                <span className="text-[11px] text-amber-700 dark:text-amber-300">
                  {t("dataDirRestartNeeded")}
                </span>
                <div className="flex shrink-0 items-center gap-1">
                  <Button size="sm" variant="primary" onPress={() => invoke("restart_app")}>
                    {t("restartNow")}
                  </Button>
                  <Button size="sm" variant="ghost" onPress={() => setPendingDataDir(false)}>
                    {t("later")}
                  </Button>
                </div>
              </div>
            )}
          </div>
        </Section>

        <Section title={t("sectionShortcut")}>
          <Row label={t("shortcutAction")}>
            <ShortcutRecordInput
              value={settings.shortcut}
              onChange={handleShortcutChange}
              onRecordingChange={handleShortcutRecordingChange}
              disabled={loadingSettings}
            />
          </Row>
        </Section>
      </main>

      <footer className="flex flex-none items-center gap-2 px-4 py-2 text-[11px] text-muted-foreground">
        <span className={`h-1.5 w-1.5 rounded-full ${statusDot}`} />
        <span className="truncate">{statusLabel || t("currentVersion") + " " + packageInfo.version}</span>
        {hasUpdate ? (
          // 查到新版本后，下载入口必须在同一处，不能让用户再去「关于」里找
          <button
            onClick={handleDownloadAndInstall}
            disabled={isDownloading || installed}
            className="ml-1 rounded-md border border-blue-500/40 bg-blue-500/10 px-2 py-[1px] font-medium text-blue-600 transition-colors hover:bg-blue-500/20 disabled:opacity-60 dark:text-blue-300"
          >
            {installed
              ? t("restarting")
              : isDownloading
                ? `${downloadProgress}%`
                : t("downloadUpdate")}
          </button>
        ) : (
          <button
            onClick={checkForUpdate}
            disabled={checkingUpdate}
            className="ml-1 rounded-md border border-zinc-200 px-2 py-[1px] transition-colors hover:bg-default disabled:opacity-50 dark:border-white/15"
          >
            {checkingUpdate ? t("checking") : t("checkUpdate")}
          </button>
        )}
        <button
          onClick={() => setAboutOpen(true)}
          className="ml-auto rounded px-1.5 py-0.5 hover:bg-default"
        >
          {t("about")}
        </button>
      </footer>
    </div>
  );
}
