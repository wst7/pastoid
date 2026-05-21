/**
 * 设置窗口应用
 */
import { useEffect, useState } from "react";
import packageInfo from '../../package.json' with { type: 'json' };
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from '@tauri-apps/api/window';
import { check, Update } from "@tauri-apps/plugin-updater";
import { Tabs, Label, ListBox, Select, Switch, Button, Spinner } from "@heroui/react";
import ShortcutRecordInput from "@/components/ShortcutRecordInput";
import { Settings, Info, RefreshCw } from "lucide-react";

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

const languages = [
  { id: "zh", name: "简体中文" },
  { id: "en", name: "English" },
];

const themes = (t: (key: string) => string) => [
  { id: "light", name: t("light") },
  { id: "dark", name: t("dark") },
];

export default function SettingsApp() {
  const { t, i18n } = useTranslation();

  const [activeTab, setActiveTab] = useState("settings");
  const [settings, setSettings] = useState<SettingsData>({
    language: "zh",
    theme: "system",
    autostart: false,
    max_items: 20,
    shortcut: "Cmd+Shift+V",
  });
  const [loadingSettings, setLoadingSettings] = useState(true);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);

  const applyTheme = (theme: string) => {
    const isDark = theme === "dark";
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
    // HeroUI Select 可能传递 Set，需要提取第一个值
    const selectedKey = (key as unknown) instanceof Set ? Array.from(key as unknown as Set<React.Key>)[0] : key;
    const theme = String(selectedKey);
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

  const handleShortcutChange = (shortcut: string) => {
    const newSettings = { ...settings, shortcut };
    setSettings(newSettings);
    saveSettings(newSettings);
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
    setDownloadProgress(0);

    try {
      await updateStatus.update.downloadAndInstall((event) => {
        switch (event.event) {
          case 'Started':
            setDownloadProgress(0);
            break;
          case 'Progress':
            // event.data.chunkLength 是当前块大小，需要手动累加计算百分比
            setDownloadProgress((prev) => {
              const newProgress = prev + (event.data.chunkLength / 1024 / 1024);
              return Math.min(newProgress, 100);
            });
            break;
          case 'Finished':
            setDownloadProgress(100);
            break;
        }
      });
      // 安装完成后会自动重启
    } catch (error) {
      console.error("Download or install failed:", error);
    } finally {
      setIsDownloading(false);
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
                <div className="w-20 h-20 mx-auto mb-4 rounded-2xl overflow-hidden shadow-sm">
                  <img src="/logo.svg" alt="Pastoid" className="w-full h-full" />
                </div>
                <h2 className="text-2xl font-bold">{t('appName')}</h2>
                <p className="text-muted-foreground mt-1">v{packageInfo.version}</p>
                <p className="text-sm text-muted-foreground mt-2">{t('description')}</p>
              </div>

              <div className="bg-card rounded-xl p-4 shadow-sm border border-border">
                <h3 className="font-medium mb-3">{t('checkUpdate')}</h3>
                {!updateStatus ? (
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
                      <span className="font-medium">{updateStatus.current_version}</span>
                    </div>
                    {updateStatus.update ? (
                      <>
                        <div className="flex justify-between items-center">
                          <span className="text-sm text-green-500">{t('newVersion')}</span>
                          <span className="font-medium text-green-500">{updateStatus.update.version}</span>
                        </div>

                        {isDownloading ? (
                          <div className="space-y-2">
                            <div className="w-full bg-muted rounded-full h-2">
                              <div
                                className="bg-primary h-2 rounded-full transition-all duration-300"
                                style={{ width: `${Math.min(downloadProgress, 100)}%` }}
                              />
                            </div>
                            <p className="text-xs text-center text-muted-foreground">
                              {t('downloading')}...
                            </p>
                          </div>
                        ) : (
                          <Button
                            onPress={handleDownloadAndInstall}
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
                        setUpdateStatus(null);
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
                <p>© 2026 Pastoid. All rights reserved.</p>
              </div>
            </div>
          </div>
        )}
      </main>
    </div>
  );
}
