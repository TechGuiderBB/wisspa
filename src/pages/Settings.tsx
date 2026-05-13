import { useEffect, useState } from "react";
import { getSettings, saveSettings, type Settings } from "../lib/settings";
import GeneralTab from "../components/settings/GeneralTab";
import ApiKeysTab from "../components/settings/ApiKeysTab";
import HotkeysTab from "../components/settings/HotkeysTab";
import ActionsTab from "../components/settings/ActionsTab";
import PromptModeTab from "../components/settings/PromptModeTab";
import HistoryTab from "../components/settings/HistoryTab";
import AboutTab from "../components/settings/AboutTab";

type TabId =
  | "general"
  | "apikeys"
  | "hotkeys"
  | "actions"
  | "prompt_mode"
  | "history"
  | "about";

const TABS: { id: TabId; label: string; icon: string; subtitle: string }[] = [
  { id: "general", label: "General", icon: "⚙", subtitle: "Launch, sounds, theme" },
  { id: "apikeys", label: "API Keys", icon: "🔑", subtitle: "Groq & Anthropic" },
  { id: "hotkeys", label: "Hotkeys", icon: "⌨", subtitle: "Triggers for each mode" },
  { id: "actions", label: "Actions", icon: "🪄", subtitle: "Voice commands" },
  { id: "prompt_mode", label: "Prompt Mode", icon: "✨", subtitle: "AI prompt rewrites" },
  { id: "history", label: "History", icon: "🕘", subtitle: "Recent dictations" },
  { id: "about", label: "About", icon: "ℹ", subtitle: "Version & permissions" },
];

export default function SettingsPage() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [active, setActive] = useState<TabId>("general");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    getSettings().then(setSettings).catch(console.error);
  }, []);

  function persist(next: Settings) {
    setSettings(next);
    setSaving(true);
    saveSettings(next)
      .catch(console.error)
      .finally(() => setSaving(false));
  }

  function patchGeneral(p: Partial<Settings["general"]>) {
    if (!settings) return;
    persist({ ...settings, general: { ...settings.general, ...p } });
  }
  function patchHotkeys(p: Partial<Settings["hotkeys"]>) {
    if (!settings) return;
    persist({ ...settings, hotkeys: { ...settings.hotkeys, ...p } });
  }
  function patchPromptMode(p: Partial<Settings["prompt_mode"]>) {
    if (!settings) return;
    persist({ ...settings, prompt_mode: { ...settings.prompt_mode, ...p } });
  }

  if (!settings) {
    return (
      <div className="h-screen w-screen flex items-center justify-center text-sm text-neutral-500 bg-neutral-50">
        Loading settings…
      </div>
    );
  }

  const activeTab = TABS.find((t) => t.id === active)!;

  return (
    <div className="h-screen w-screen flex bg-gradient-to-br from-slate-50 via-white to-blue-50 text-neutral-900">
      <aside className="w-56 shrink-0 border-r border-neutral-200/80 bg-white/70 backdrop-blur-sm p-2 flex flex-col">
        <div className="px-3 py-3 flex items-center gap-2">
          <div className="h-7 w-7 rounded-lg bg-gradient-to-br from-blue-500 to-violet-500 flex items-center justify-center text-white text-xs font-bold shadow-sm">
            W
          </div>
          <div>
            <div className="text-sm font-semibold leading-tight">Wisspa</div>
            <div className="text-[10px] text-neutral-500 leading-tight">
              v0.1.0
            </div>
          </div>
        </div>
        <nav className="mt-2 flex-1 space-y-0.5">
          {TABS.map((t) => {
            const isActive = active === t.id;
            return (
              <button
                key={t.id}
                type="button"
                onClick={() => setActive(t.id)}
                className={`group w-full text-left rounded-lg px-2.5 py-2 text-sm transition-all flex items-center gap-2.5 ${
                  isActive
                    ? "bg-gradient-to-r from-blue-500 to-violet-500 text-white shadow-sm"
                    : "hover:bg-neutral-100 text-neutral-700"
                }`}
              >
                <span
                  className={`text-base ${
                    isActive ? "" : "opacity-60 group-hover:opacity-100"
                  }`}
                >
                  {t.icon}
                </span>
                <span className="font-medium">{t.label}</span>
              </button>
            );
          })}
        </nav>
        <div className="px-3 py-2 text-[10px] text-neutral-400 h-5">
          {saving ? "Saving…" : ""}
        </div>
      </aside>

      <main className="flex-1 overflow-y-auto">
        <header className="sticky top-0 z-10 px-8 pt-8 pb-4 bg-gradient-to-br from-slate-50/95 via-white/95 to-blue-50/95 backdrop-blur-md border-b border-neutral-200/60">
          <div className="flex items-baseline gap-3">
            <h1 className="text-2xl font-bold tracking-tight">
              {activeTab.label}
            </h1>
            <span className="text-sm text-neutral-500">{activeTab.subtitle}</span>
          </div>
        </header>

        <div className="px-8 py-6">
          {active === "general" && (
            <GeneralTab settings={settings} patch={patchGeneral} />
          )}
          {active === "apikeys" && <ApiKeysTab />}
          {active === "hotkeys" && (
            <HotkeysTab settings={settings} patch={patchHotkeys} />
          )}
          {active === "actions" && <ActionsTab />}
          {active === "prompt_mode" && (
            <PromptModeTab settings={settings} patch={patchPromptMode} />
          )}
          {active === "history" && <HistoryTab />}
          {active === "about" && <AboutTab />}
        </div>
      </main>
    </div>
  );
}
