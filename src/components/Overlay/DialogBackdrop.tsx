interface DialogBackdropProps {
  ariaLabel: string;
  onDismiss: () => void;
  className: string;
}

export function DialogBackdrop({
  ariaLabel,
  onDismiss,
  className,
}: DialogBackdropProps) {
  return (
    <button
      type="button"
      tabIndex={-1}
      aria-label={ariaLabel}
      onClick={onDismiss}
      className={className}
    />
  );
}
