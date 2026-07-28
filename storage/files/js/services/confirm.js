import './sp-confirm-dialog.js';

let dialogEl = null;

const getDialog = () => {
  if (!dialogEl) {
    dialogEl = document.createElement('sp-confirm-dialog');
    document.body.append(dialogEl);
  }
  return dialogEl;
};

export const showConfirmDialog = async (title, message, confirmLabel, onConfirm, opts = {}) => {
  const result = await getDialog().confirm(title, message, confirmLabel, { primary: opts.btnClass === 'btn-primary' });
  if (result) onConfirm();
};

export const showPromptDialog = async (title, message, defaultValue, onSubmit) => {
  const value = await getDialog().prompt(title, message, defaultValue);
  if (value) onSubmit(value);
};
