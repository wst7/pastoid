import { useState, useCallback, useRef, useEffect } from "react";
import { Kbd } from "@heroui/react";

interface Props {
  value: string;
  onChange: (shortcut: string) => void;
  disabled?: boolean;
}

const isMac = typeof navigator !== "undefined" && navigator.platform.toUpperCase().indexOf("MAC") >= 0;

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

export default function ShortcutRecordInput({ value, onChange, disabled }: Props) {
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

    window.addEventListener("keydown", handleKeyDown, true);
    window.addEventListener("keyup", handleKeyUp, true);
    return () => {
      window.removeEventListener("keydown", handleKeyDown, true);
      window.removeEventListener("keyup", handleKeyUp, true);
    };
  }, [recording, finishRecording]);

  return (
    <button
      type="button"
      disabled={disabled}
      onClick={() => {
        if (!disabled && !recording) {
          setRecording(true);
          setPendingDisplay("");
        }
      }}
      className={`inline-flex items-center gap-1.5 px-3 py-2 rounded-lg border text-sm transition-colors select-none ${
        disabled
          ? "border-zinc-200 dark:border-zinc-700 text-zinc-400 cursor-not-allowed"
          : recording
            ? "border-blue-400 ring-2 ring-blue-400/30 bg-blue-50 dark:bg-blue-500/10 text-blue-600"
            : "border-zinc-200 dark:border-zinc-700 hover:border-zinc-300 dark:hover:border-zinc-600 cursor-pointer text-zinc-700 dark:text-zinc-300"
      }`}
    >
      {recording ? (
        <span>
          {pendingDisplay || (
            <span className="text-zinc-400 animate-pulse">Recording…</span>
          )}
        </span>
      ) : (
        <span className="flex items-center gap-1">
          {shortcutToDisplay(value).split("").map((ch, i) => (
            <Kbd
              key={i}
              className="bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300 text-xs px-1.5 py-0.5"
            >
              {ch}
            </Kbd>
          ))}
        </span>
      )}
    </button>
  );
}
