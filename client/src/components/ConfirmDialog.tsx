import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, Modal } from "./ui";

/**
 * In-app modal confirm dialog. Replaces `window.confirm()` everywhere
 * because Tauri's macOS WKWebView silently drops `confirm()` calls —
 * the dialog never shows, the call returns `false`, and the user is
 * stuck unable to action destructive buttons. v0.3.1 shipped with the
 * native call still in place; v0.3.2 routes everything through here.
 *
 * Usage:
 *
 * ```tsx
 * const { confirm, dialog } = useConfirm();
 *
 * async function handleDiscard() {
 *   if (!(await confirm({ message: t("...") }))) return;
 *   await invoke("flight_cancel");
 * }
 *
 * return <>
 *   {dialog}
 *   <button onClick={handleDiscard}>Discard</button>
 * </>;
 * ```
 *
 * The hook keeps the call-site change minimal — only `confirm(msg)`
 * becomes `await confirm({ message: msg })` plus mounting `{dialog}`.
 * The dialog itself renders nothing until a confirm is in flight.
 */

interface ConfirmOptions {
  message: string;
  /** Optional title above the message. Defaults to the i18n key
   *  `confirm_dialog.default_title` (= "Bist du sicher?" / "Are you sure?"). */
  title?: string;
  /** Confirm-button label. Defaults to i18n `confirm_dialog.confirm`. */
  confirmLabel?: string;
  /** Cancel-button label. Defaults to i18n `confirm_dialog.cancel`. */
  cancelLabel?: string;
  /** Treat the action as destructive (red confirm button). */
  destructive?: boolean;
}

interface PendingConfirm extends ConfirmOptions {
  resolve: (ok: boolean) => void;
}

export function useConfirm() {
  const { t } = useTranslation();
  const [pending, setPending] = useState<PendingConfirm | null>(null);
  // Stable resolver ref so the dialog's button handlers don't rebind
  // every render (would cause focus to jump on key state changes).
  const pendingRef = useRef<PendingConfirm | null>(null);
  pendingRef.current = pending;

  const confirm = useCallback((opts: ConfirmOptions): Promise<boolean> => {
    return new Promise((resolve) => {
      setPending({ ...opts, resolve });
    });
  }, []);

  const close = useCallback((ok: boolean) => {
    const cur = pendingRef.current;
    if (cur) cur.resolve(ok);
    setPending(null);
  }, []);

  // Enter bestätigt — das erwarten Nutzer vom nativen `confirm()`.
  // Escape, Fokus-Falle, Fokus-Rückgabe und Klick auf den Schleier
  // übernimmt seit Stufe D die Modal-Primitive.
  //
  // v0.19.x FIX: das galt bisher UNBEDINGT, auch wenn der Cancel-Button
  // (bewusst per `autoFocus` als sicherer Default bei destruktiven
  // Aktionen gesetzt) den Fokus hatte — Enter hat dann trotzdem
  // bestätigt statt abzubrechen. Ein fokussierter `<button>` reagiert
  // bereits nativ auf Enter (löst seinen eigenen `click` aus); wir
  // dürfen das nicht überschreiben. Der Fallback (`close(true)`) greift
  // nur, wenn der Fokus (noch) NICHT auf einem der beiden Dialog-Buttons
  // liegt — z.B. im kurzen Fenster bevor die Modal-Fokus-Falle greift.
  useEffect(() => {
    if (!pending) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Enter" || e.repeat) return;
      const active = document.activeElement;
      if (active instanceof HTMLButtonElement) return;
      e.preventDefault();
      close(true);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pending, close]);

  const dialog = (
    <Modal
      open={pending !== null}
      onClose={() => close(false)}
      size="sm"
      title={pending?.title ?? t("confirm_dialog.default_title")}
      closeLabel={t("confirm_dialog.cancel")}
      footer={
        <>
          <Button onClick={() => close(false)} autoFocus>
            {pending?.cancelLabel ?? t("confirm_dialog.cancel")}
          </Button>
          <Button
            variant={pending?.destructive ? "danger" : "primary"}
            onClick={() => close(true)}
          >
            {pending?.confirmLabel ?? t("confirm_dialog.confirm")}
          </Button>
        </>
      }
    >
      <p className="confirm-dialog__message">{pending?.message}</p>
    </Modal>
  );

  return { confirm, dialog };
}
