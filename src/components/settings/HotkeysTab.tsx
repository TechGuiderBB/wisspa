import { Settings } from "../../lib/settings";
import { HotkeyEditor } from "../HotkeyEditor";

type Props = {
  settings: Settings;
  patch: (p: Partial<Settings["hotkeys"]>) => void;
};

export default function HotkeysTab({ settings, patch }: Props) {
  return <HotkeyEditor hotkeys={settings.hotkeys} onPatch={patch} />;
}
