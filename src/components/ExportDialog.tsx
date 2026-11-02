import { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import { useStore } from "../lib/store";

/**
 * "Copy for AI": a Markdown report built by the backend (export.rs), shown so
 * it can be copied (or saved / shared) and pasted into Claude or ChatGPT.
 * The text is also selectable, for webviews that block the clipboard.
 */
export function ExportDialog({ load, filename, onClose }: { load: () => Promise<string>; filename: string; onClose: () => void }) {
  const mobile = useStore((s) => s.build?.mobile ?? false);
  const [text, setText] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    load()
      .then(setText)
      .catch((e) => setError(String(e)));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const copy = async () => {
    if (text == null) return;
    try {
      await navigator.clipboard.writeText(text);
      setNote("Copied. Paste it into Claude or ChatGPT.");
    } catch {
      // Fallback: select the text so it can be copied by hand.
      const el = document.getElementById("an-export-text") as HTMLTextAreaElement | null;
      el?.focus();
      el?.select();
      setNote("Couldn't reach the clipboard: the text is selected, copy it from there.");
    }
  };
  const saveFile = async () => {
    if (text == null) return;
    try {
      const path = await save({ defaultPath: filename, filters: [{ name: "Markdown", extensions: ["md"] }] });
      if (!path) return;
      await api.saveText(path, text);
      setNote(`Saved ${path.split(/[\\/]/).pop()}.`);
    } catch (e) {
      setError(String(e));
    }
  };
  const share = async () => {
    if (text == null) return;
    try {
      await navigator.share({ title: filename, text });
    } catch {
      /* cancelled */
    }
  };
  const canShare = typeof navigator.share === "function";
  const btn = "tech rounded-[10px] px-4 py-2 text-[13px] font-semibold";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4" onClick={onClose}>
      <div
        className="flex max-h-full w-full max-w-3xl flex-col rounded-[16px] border border-accent/50 bg-[#070b0a] p-5 shadow-[0_0_24px_rgba(57,255,122,0.15)] max-sm:p-4"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2 className="tech text-[18px] font-bold text-accent">Copy for AI</h2>
            <p className="mt-1 text-[13px] text-white/60">
              Paste this into Claude or ChatGPT and ask where you're losing time. It already asks the AI to coach you; add your own
              questions after it.
            </p>
          </div>
          <button onClick={onClose} className="tech shrink-0 text-[13px] text-white/50 hover:text-white">
            Close
          </button>
        </div>
        {error ? (
          <div className="mt-4 text-[14px] text-red-300">{error}</div>
        ) : (
          <textarea
            id="an-export-text"
            readOnly
            value={text ?? "Building the report…"}
            className="num scroll-thin mt-4 min-h-[200px] flex-1 resize-none rounded-[10px] border border-white/10 bg-black/60 p-3 text-[12px] leading-snug text-white/80 outline-none"
            style={{ height: "50vh" }}
          />
        )}
        <div className="mt-4 flex flex-wrap items-center justify-end gap-2">
          {note && <span className="mr-auto text-[13px] text-accent">{note}</span>}
          {canShare && (
            <button onClick={share} disabled={text == null} className={`${btn} border border-white/15 text-white/80`}>
              Share
            </button>
          )}
          {!mobile && (
            <button onClick={saveFile} disabled={text == null} className={`${btn} border border-white/15 text-white/80`}>
              Save file
            </button>
          )}
          <button onClick={copy} disabled={text == null} className={`${btn} bg-accent text-black`}>
            Copy
          </button>
        </div>
      </div>
    </div>
  );
}
