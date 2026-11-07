import { useState, useCallback, useRef, useEffect } from "react";
import { Kbd } from "@heroui/react";
import { isMac } from "@/lib/platform";

interface Props {
  value: string;
  onChange: (shortcut: string) => void;
  /** 录制开始/结束通知：调用方据此暂停全局热键，否则按下的组合会先被热键抢走 */
  onRecordingChange?: (recording: boolean) => void;
  disabled?: boolean;
}

function shortcutToDisplay(s: string): string {
  return s
    .split("+")
    .map((part) => {
      const p = part.trim();
      if (isMac) {
        switch (p.toLowerCase()) {
          case "cmd": return "\u2318";
          case "shift": return "\u21E7";
          case "ctrl": return "\u2303";
          case "alt": return "\u2325";
          default: return p.toUpperCase();
        }
      }
      return p.charAt(0).toUpperCase() + p.slice(1).toLowerCase();
    })
    .join(isMac ? "" : "+");
}

function codeToKey(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  return code;
}

export default function ShortcutRecordInput({ value, onChange, onRecordingChange, disabled }: Props) {
  const [recording, setRecording] = useState(false);
  const [pendingDisplay, setPendingDisplay] = useState("");
  const keysRef = useRef<{ modifiers: string[]; key: string | null }>({
    modifiers: [],
    key: null,
  });

  const finishRecording = useCallback(() => {
    const { modifiers, key } = keysRef.current;
    if (modifiers.length > 0 && key) {
      onChange([...modifiers, key].join("+"));
    }
    setRecording(false);
    setPendingDisplay("");
    keysRef.current = { modifiers: [], key: null };
  }, [onChange]);

  useEffect(() => {
    if (!recording) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const { modifiers: prevMods, key: prevKey } = keysRef.current;

      if (e.code === "Escape") {
        setRecording(false);
        setPendingDisplay("");
        keysRef.current = { modifiers: [], key: null };
        onRecordingChange?.(false);
        return;
      }

      const newModifiers: string[] = [];
      if (e.metaKey) newModifiers.push("Cmd");
      if (e.ctrlKey) newModifiers.push("Ctrl");
      if (e.altKey) newModifiers.push("Alt");
      if (e.shiftKey) newModifiers.push("Shift");

      const modifierCodes = [
        "MetaLeft", "MetaRight", "ControlLeft", "ControlRight",
        "ShiftLeft", "ShiftRight", "AltLeft", "AltRight",
      ];
      let mainKey: string | null = null;
      if (!modifierCodes.includes(e.code)) {
        mainKey = codeToKey(e.code);
      }

      keysRef.current = {
        modifiers: newModifiers.length > 0 ? newModifiers : prevMods,
        key: mainKey || prevKey,
      };

      const displayMods = newModifiers.length > 0 ? newModifiers : prevMods;
      const displayKey = mainKey || prevKey;
      if (displayMods.length > 0 && displayKey) {
        setPendingDisplay(shortcutToDisplay(displayMods.join("+") + "+" + displayKey));
      } else if (displayMods.length > 0) {
        setPendingDisplay(shortcutToDisplay(displayMods.join("+")));
      }
    };

    const handleKeyUp = () => {
      const { modifiers, key } = keysRef.current;
      if (modifiers.length > 0 && key) {
        finishRecording();
      }
    };

    // 录制中窗口失去焦点（切走、点别处）也要收尾，否则全局热键会一直停着
    const handleBlur = () => {
      setRecording(false);
      setPendingDisplay("");
      keysRef.current = { modifiers: [], key: null };
      onRecordingChange?.(false);
    };

    window.addEventListener("keydown", handleKeyDown, true);
    window.addEventListener("keyup", handleKeyUp, true);
    window.addEventListener("blur", handleBlur);
    return () => {
      window.removeEventListener("keydown", handleKeyDown, true);
      window.removeEventListener("keyup", handleKeyUp, true);
      window.removeEventListener("blur", handleBlur);
    };
  }, [recording, finishRecording, onRecordingChange]);

  return (
    <button
      type="button"
      disabled={disabled}
      onClick={() => {
        if (!disabled && !recording) {
          setRecording(true);
          setPendingDisplay("");
          onRecordingChange?.(true);
        }
      }}
      className={`inline-flex select-none items-center gap-1.5 rounded-lg px-1.5 py-1 text-sm transition-colors ${
        disabled
          ? "cursor-not-allowed text-muted-foreground/60"
          : recording
            ? "bg-primary/10 text-primary ring-2 ring-primary/40"
            : "cursor-pointer hover:bg-default"
      }`}
    >
      {recording ? (
        <span>
          {pendingDisplay || (
            <span className="text-muted-foreground/60 animate-pulse">Recording…</span>
          )}
        </span>
      ) : (
        <span className="flex items-center gap-1">
          {(isMac ? shortcutToDisplay(value).split("") : shortcutToDisplay(value).split("+")).map(
            (part, i) => (
              <Kbd
                key={i}
                className="bg-default px-1.5 py-0.5 text-xs text-foreground"
              >
                {part}
              </Kbd>
            )
          )}
        </span>
      )}
    </button>
  );
}
