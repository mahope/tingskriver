import React from 'react';

interface ConfirmationModalProps {
  onConfirm: () => void;
  onCancel: () => void;
  text: string;
  isOpen: boolean;
}

export function ConfirmationModal({ onConfirm, onCancel, text, isOpen }: ConfirmationModalProps) {
  if (!isOpen) return null;

  return (
    <div role="dialog" aria-modal="true" aria-labelledby="confirm-title" className="fixed inset-0 bg-foreground/50 flex items-center justify-center z-50">
      <div className="bg-card rounded-lg p-6 max-w-md w-full mx-4">
        <h2 id="confirm-title" className="text-xl font-semibold mb-4">Bekræft sletning</h2>
        <p className="text-muted-foreground mb-6">{text}</p>
        <div className="flex justify-end space-x-4">
          <button
            onClick={onCancel}
            className="px-4 py-2 min-h-[44px] text-muted-foreground hover:bg-accent rounded-md transition-colors"
          >
            Annullér
          </button>
          <button
            onClick={onConfirm}
            className="px-4 py-2 min-h-[44px] bg-destructive text-destructive-foreground hover:bg-destructive/90 rounded-md transition-colors"
          >
            Slet
          </button>
        </div>
      </div>
    </div>
  );
}
