import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { useCallback, useEffect, useRef } from "react";
import type { ReactNode, RefObject } from "react";

export type DialogSize = "sm" | "md" | "lg" | "xl" | "full";

export type DialogProps = {
  title: string;
  subtitle?: string;
  label?: string;
  closeLabel: string;
  onClose: () => void;
  dismissible?: boolean;
  size?: DialogSize;
  showClose?: boolean;
  headerActions?: ReactNode;
  footer?: ReactNode;
  className?: string;
  bodyClassName?: string;
  initialFocusRef?: RefObject<HTMLElement | null>;
  children: ReactNode;
};

const FOCUSABLE_SELECTOR =
  'button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),a[href],summary,[tabindex]:not([tabindex="-1"])';

const dialogStack: symbol[] = [];

export function Dialog({
  title,
  subtitle,
  label,
  closeLabel,
  onClose,
  dismissible = true,
  size = "md",
  showClose = true,
  headerActions,
  footer,
  className,
  bodyClassName,
  initialFocusRef,
  children,
}: DialogProps) {
  const contentRef = useRef<HTMLDivElement>(null);
  const cleanupRef = useRef<(() => void) | null>(null);
  const dialogId = useRef(Symbol("dialog"));

  const setContentRef = useCallback(
    (node: HTMLDivElement | null) => {
      contentRef.current = node;
      cleanupRef.current?.();
      cleanupRef.current = null;
      if (!node) return;

      const id = dialogId.current;
      dialogStack.push(id);
      const isTop = () => dialogStack[dialogStack.length - 1] === id;
      const previous = document.activeElement as HTMLElement | null;
      const focusables = () =>
        Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
      const focusFirst = () => {
        const target =
          initialFocusRef?.current ??
          node
            .querySelector<HTMLElement>(".ui-dialog-body")
            ?.querySelector<HTMLElement>(FOCUSABLE_SELECTOR) ??
          focusables()[0] ??
          node;
        target.focus();
      };
      const guard = (event: FocusEvent) => {
        if (isTop() && !node.contains(event.target as Node)) focusFirst();
      };
      const trap = (event: KeyboardEvent) => {
        if (!isTop() || event.key !== "Tab") return;
        const nodes = focusables();
        if (!nodes.length) return;
        const first = nodes[0];
        const last = nodes[nodes.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      };

      focusFirst();
      document.addEventListener("focusin", guard);
      node.addEventListener("keydown", trap);
      cleanupRef.current = () => {
        document.removeEventListener("focusin", guard);
        node.removeEventListener("keydown", trap);
        const index = dialogStack.lastIndexOf(id);
        const wasTop = index === dialogStack.length - 1;
        if (index >= 0) dialogStack.splice(index, 1);
        if (wasTop && previous?.isConnected) previous.focus();
      };
    },
    [initialFocusRef],
  );

  useEffect(
    () => () => {
      cleanupRef.current?.();
      cleanupRef.current = null;
    },
    [],
  );

  return (
    <DialogPrimitive.Root
      open
      modal={false}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="ui-overlay" />
        <DialogPrimitive.Content
          ref={setContentRef}
          className={`ui-dialog ui-dialog-${size}${className ? ` ${className}` : ""}`}
          aria-label={label ?? title}
          aria-labelledby={undefined}
          aria-modal="true"
          onEscapeKeyDown={(event) => {
            if (!dismissible) event.preventDefault();
          }}
          onPointerDownOutside={(event) => event.preventDefault()}
          onInteractOutside={(event) => event.preventDefault()}
          onOpenAutoFocus={(event) => {
            const target =
              initialFocusRef?.current ??
              contentRef.current
                ?.querySelector<HTMLElement>(".ui-dialog-body")
                ?.querySelector<HTMLElement>(FOCUSABLE_SELECTOR);
            if (target) {
              event.preventDefault();
              target.focus();
            }
          }}
        >
          <header className="ui-dialog-header">
            <div className="ui-dialog-heading">
              <DialogPrimitive.Title className="ui-dialog-title">{title}</DialogPrimitive.Title>
              {subtitle ? (
                <DialogPrimitive.Description className="ui-dialog-subtitle">
                  {subtitle}
                </DialogPrimitive.Description>
              ) : null}
            </div>
            {headerActions ? <div className="ui-dialog-header-actions">{headerActions}</div> : null}
            {showClose ? (
              <DialogPrimitive.Close asChild>
                <button
                  type="button"
                  className="ui-dialog-close"
                  aria-label={closeLabel}
                  disabled={!dismissible}
                >
                  <X size={18} aria-hidden="true" />
                </button>
              </DialogPrimitive.Close>
            ) : null}
          </header>
          <div className={`ui-dialog-body${bodyClassName ? ` ${bodyClassName}` : ""}`}>
            {children}
          </div>
          {footer ? <footer className="ui-dialog-footer">{footer}</footer> : null}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
