import { useEffect, useRef, type ReactNode } from "react";

export function Dialog({ title, children, onClose, className = "" }: {
  title: string; children: ReactNode; onClose: () => void; className?: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  return <dialog ref={ref} aria-label={title} className={`modal-card ${className}`} onCancel={event => {
    event.preventDefault();
    onClose();
  }}>{children}</dialog>;
}
