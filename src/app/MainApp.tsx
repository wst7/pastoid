/**
 * 主窗口应用 - 剪贴板管理 + 设置
 */
import { useEffect, useState } from "react";
import packageInfo from '../../package.json' with { type: 'json' };
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { SearchBar } from "@/components/SearchBar";
import { ClipboardList } from "@/components/ClipboardList";
import ShortcutRecordInput from "@/components/ShortcutRecordInput";
import { useClipboard } from "@/hooks/useClipboard";
import { Tabs, Label, ListBox, Select, Switch, Button, Spinner } from "@heroui/react";
import { ClipboardList as ClipboardListIcon, Settings, Info, RefreshCw } from "lucide-react";

interface UpdateInfo {
  has_update: boolean;
  current_version: string;
  latest_version: string;
  download_url: string;
  release_notes: string | null;
}

interface DownloadProgress {
  downloaded: number;
  total: number;
  percent: number;
}

interface SettingsData {
  language: string;
  theme: string;
  autostart: boolean;
  max_items: number;
  shortcut: string;
}

const languages = [
  { id: "zh", name: "简体中文" },
  { id: "en", name: "English" },
];

const themes = (t: (key: string) => string) => [
  { id: "light", name: t("light") },
  { id: "dark", name: t("dark") },
];

export default function App() {
  const { t, i18n } = useTranslation();
  const { displayedPinned, displayedUnpinned, isLoading, search, setSearch, copyToClipboard, deleteItem, togglePin, reload } = useClipboard();

  const [activeTab, setActiveTab] = useState("clips");
  const [settings, setSettings] = useState<SettingsData>({
    language: "zh",
    theme: "system",
    autostart: false,
    max_items: 20,
    shortcut: "Cmd+Shift+V",
  });
  const [loadingSettings, setLoadingSettings] = useState(true);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [downloadedPath, setDownloadedPath] = useState<string | null>(null);

  const applyTheme = (theme: string) => {
    const isDark =
      theme === "dark" ||
      (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
    const html = document.documentElement;

    const currentWindow = getCurrentWindow();
    if (typeof currentWindow.setTheme === 'function') {
      currentWindow.setTheme(isDark ? 'dark' : 'light');
    }

    if (isDark) {
      html.classList.add("dark");
      html.classList.remove("light");
      html.setAttribute("data-theme", "dark");
    } else {
      html.classList.remove("dark");
      html.classList.add("light");
      html.setAttribute("data-theme", "light");
    }
  };

  useEffect(() => {
    applyTheme(settings.theme);
  }, [settings.theme]);

  useEffect(() => {
    loadSettings();
  }, []);

  const loadSettings = async () => {
    try {
      const data = await invoke<SettingsData>("get_settings");
      setSettings({
        language: data.language || "zh",
        theme: data.theme || "light",
        autostart: data.autostart ?? false,
        max_items: data.max_items ?? 20,
        shortcut: data.shortcut || "Cmd+Shift+V",
      });
      applyTheme(data.theme || "light");
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
      applyTheme(newSettings.theme);
    } catch (error) {
      console.error("Failed to save settings:", error);
    }
  };

  const handleLanguageChange = (key: React.Key | null) => {
    if (!key) return;
    const language = key as string;
    const newSettings = { ...settings, language };
    setSettings(newSettings);
    i18n.changeLanguage(language === "zh" ? "zh" : "en");
    saveSettings(newSettings);
  };

  const handleThemeChange = (key: React.Key | null) => {
    if (!key) return;
    const theme = key as string;
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
    reload();
  };

  const handleShortcutChange = (shortcut: string) => {
    const newSettings = { ...settings, shortcut };
    setSettings(newSettings);
    saveSettings(newSettings);
  };

  const checkForUpdate = async () => {
    setCheckingUpdate(true);
    try {
      const info = await invoke<UpdateInfo>("check_update");
      setUpdateInfo(info);
    } catch (error) {
      console.error("Failed to check update:", error);
    } finally {
      setCheckingUpdate(false);
    }
  };

  const handleDownloadUpdate = async () => {
    setIsDownloading(true);
    setDownloadProgress(0);

    const unlisten = await listen<DownloadProgress>('download_progress', (event) => {
      setDownloadProgress(event.payload.percent);
    });

    try {
      const path = await invoke<string>("start_download_update");
      setDownloadedPath(path);
    } catch (error) {
      console.error("Download failed:", error);
      if (updateInfo?.download_url) {
        openUrl(updateInfo.download_url);
      }
    } finally {
      setIsDownloading(false);
      unlisten();
    }
  };

  const handleInstall = () => {
    if (downloadedPath) {
      invoke("open_installer", { path: downloadedPath });
    }
  };

  const selectedLanguage = languages.find((l) => l.id === settings.language);
  const themeOptions = themes(t);
  const selectedTheme = themeOptions.find((th) => th.id === settings.theme);

  return (
    <div className="min-h-screen bg-background text-foreground flex flex-col">
      <div className="sticky top-0 z-20 bg-background pt-[38px] text-foreground">
        <Tabs
          selectedKey={activeTab}
          onSelectionChange={(key) => setActiveTab(String(key))}
          className="px-4"
        >
          <Tabs.ListContainer>
            <Tabs.List aria-label="Navigation">
              <Tabs.Tab id="clips">
                <div className="flex items-center gap-2">
                  <ClipboardListIcon className="size-4" />
                  <span>{t('clips')}</span>
                </div>
                <Tabs.Indicator />
              </Tabs.Tab>
              <Tabs.Tab id="settings">
                <div className="flex items-center gap-2">
                  <Settings className="size-4" />
                  <span>{t('settings')}</span>
                </div>
                <Tabs.Indicator />
              </Tabs.Tab>
              <Tabs.Tab id="about">
                <div className="flex items-center gap-2">
                  <Info className="size-4" />
                  <span>{t('about')}</span>
                </div>
                <Tabs.Indicator />
              </Tabs.Tab>
            </Tabs.List>
          </Tabs.ListContainer>
        </Tabs>
      </div>

      <main className="flex-1 overflow-hidden">
        {activeTab === "clips" && (
          <div className="h-full flex flex-col">
            <div className="px-4 py-3 border-b border-border bg-background">
              <SearchBar value={search} onChange={setSearch} placeholder={t('search')} />
            </div>
            <div className="flex-1 overflow-hidden">
              {isLoading ? (
                <div className="flex items-center justify-center h-full">
                  <div className="w-5 h-5 border border-blue-500 border-t-transparent rounded-full animate-spin" />
                </div>
              ) : (
                <ClipboardList
                  pinnedItems={displayedPinned}
                  unpinnedItems={displayedUnpinned}
                  onCopy={copyToClipboard}
                  onDelete={deleteItem}
                  onTogglePin={togglePin}
                />
              )}
            </div>
          </div>
        )}

        {activeTab === "settings" && (
          <div className="h-full overflow-y-auto p-4 space-y-6">
            <div className="grid gap-1.5">
              <Label>{t('language')}</Label>
              <Select
                selectedKey={settings.language}
                onSelectionChange={handleLanguageChange}
                isDisabled={loadingSettings}
              >
                <Select.Trigger>
                  <Select.Value>
                    {selectedLanguage?.name || t('selectLanguage')}
                  </Select.Value>
                  <Select.Indicator />
                </Select.Trigger>
                <Select.Popover>
                  <ListBox>
                    {languages.map((lang) => (
                      <ListBox.Item key={lang.id} id={lang.id} textValue={lang.name}>
                        {lang.name}
                      </ListBox.Item>
                    ))}
                  </ListBox>
                </Select.Popover>
              </Select>
            </div>

            <div className="grid gap-1.5">
              <Label>{t('theme')}</Label>
              <Select
                selectedKey={settings.theme}
                onSelectionChange={handleThemeChange}
                isDisabled={loadingSettings}
              >
                <Select.Trigger>
                  <Select.Value>
                    {selectedTheme?.name || t('selectTheme')}
                  </Select.Value>
                  <Select.Indicator />
                </Select.Trigger>
                <Select.Popover>
                  <ListBox>
                    {themeOptions.map((theme) => (
                      <ListBox.Item key={theme.id} id={theme.id} textValue={theme.name}>
                        {theme.name}
                      </ListBox.Item>
                    ))}
                  </ListBox>
                </Select.Popover>
              </Select>
            </div>

            <div className="flex flex-row items-center justify-between">
              <Label>{t('autoStart')}</Label>
              <Switch
                isSelected={settings.autostart}
                onChange={handleAutostartChange}
                isDisabled={loadingSettings}
              >
                <Switch.Control>
                  <Switch.Thumb />
                </Switch.Control>
              </Switch>
            </div>

            <div className="flex flex-row items-center justify-between">
              <Label>{t('maxItems')}</Label>
              <input
                type="number"
                min={1}
                max={100}
                value={String(settings.max_items)}
                onChange={(e) => handleMaxItemsChange(e.target.value)}
                disabled={loadingSettings}
                className="w-40 px-3 py-2 rounded-lg border border-input bg-background text-sm focus:outline-none focus:ring-2 focus:ring-ring"
              />
            </div>

            <div className="flex flex-row items-center justify-between">
              <Label>{t('shortcutLabel')}</Label>
              <ShortcutRecordInput
                value={settings.shortcut}
                onChange={handleShortcutChange}
                disabled={loadingSettings}
              />
            </div>
          </div>
        )}

        {activeTab === "about" && (
          <div className="h-full overflow-y-auto">
            <div className="max-w-sm mx-auto p-6 space-y-8">
              <div className="text-center">
                <div className="w-20 h-20 mx-auto mb-4 rounded-2xl bg-gradient-to-br from-primary/20 to-primary/5 flex items-center justify-center">
                  <ClipboardListIcon className="size-10 text-primary" />
                </div>
                <h2 className="text-2xl font-bold">{t('appName')}</h2>
                <p className="text-muted-foreground mt-1">v{packageInfo.version}</p>
                <p className="text-sm text-muted-foreground mt-2">Lightweight clipboard manager</p>
              </div>

              <div className="bg-card rounded-xl p-4 shadow-sm border border-border">
                <h3 className="font-medium mb-3">{t('checkUpdate')}</h3>
                {!updateInfo ? (
                  <Button
                    onPress={checkForUpdate}
                    isDisabled={checkingUpdate}
                    variant="primary"
                    className="w-full"
                  >
                    {checkingUpdate ? <Spinner size="sm" /> : <RefreshCw className="w-4 h-4" />}
                    {checkingUpdate ? t('checking') : t('checkUpdate')}
                  </Button>
                ) : (
                  <div className="space-y-3">
                    <div className="flex justify-between items-center">
                      <span className="text-sm text-muted-foreground">{t('currentVersion')}</span>
                      <span className="font-medium">{updateInfo.current_version}</span>
                    </div>
                    {updateInfo.has_update ? (
                      <>
                        <div className="flex justify-between items-center">
                          <span className="text-sm text-green-500">{t('newVersion')}</span>
                          <span className="font-medium text-green-500">{updateInfo.latest_version}</span>
                        </div>

                        {isDownloading ? (
                          <div className="space-y-2">
                            <div className="w-full bg-muted rounded-full h-2">
                              <div
                                className="bg-primary h-2 rounded-full transition-all duration-300"
                                style={{ width: `${downloadProgress}%` }}
                              />
                            </div>
                            <p className="text-xs text-center text-muted-foreground">
                              {t('downloading')} {downloadProgress.toFixed(1)}%
                            </p>
                          </div>
                        ) : downloadedPath ? (
                          <Button
                            onPress={handleInstall}
                            variant="primary"
                            className="w-full"
                          >
                            {t('installUpdate')}
                          </Button>
                        ) : (
                          <Button
                            onPress={handleDownloadUpdate}
                            variant="primary"
                            className="w-full"
                          >
                            {t('downloadUpdate')}
                          </Button>
                        )}
                      </>
                    ) : (
                      <div className="text-center text-sm text-muted-foreground py-2">{t('upToDate')}</div>
                    )}
                    <Button
                      variant="ghost"
                      onPress={() => {
                        setUpdateInfo(null);
                        setDownloadedPath(null);
                        setDownloadProgress(0);
                      }}
                      className="w-full"
                    >
                      {t('recheck')}
                    </Button>
                  </div>
                )}
              </div>

              <div className="text-center text-xs text-muted-foreground pt-4">
                <p>© 2026 ClipOn. All rights reserved.</p>
              </div>
            </div>
          </div>
        )}
      </main>
    </div>
  );
}
