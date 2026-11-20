const EDIT_TRIGGER_KEY = /^[a-zA-Z[\]]$/;

export function isEditTriggerKey(key) {
  return typeof key === 'string' && key.length === 1 && EDIT_TRIGGER_KEY.test(key);
}
