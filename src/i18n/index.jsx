import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import { zh } from './zh';

const I18nContext = createContext(null);
const STORAGE_KEY = 'language';

function initialLanguage() {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === 'zh' || saved === 'en') return saved;
  } catch {
    // Use the system language when storage is unavailable.
  }
  return typeof navigator !== 'undefined' && navigator.language?.toLowerCase().startsWith('zh')
    ? 'zh'
    : 'en';
}

function interpolate(text, values) {
  if (!values) return text;
  return text.replace(/\{(\w+)\}/g, (match, key) =>
    Object.hasOwn(values, key) ? String(values[key]) : match,
  );
}

export function translate(locale, text, values) {
  if (typeof text !== 'string') return text;
  const translated = locale === 'zh' ? zh[text] || text : text;
  return interpolate(translated, values);
}

export function I18nProvider({ children }) {
  const [locale, setLocaleState] = useState(initialLanguage);
  const setLocale = useCallback((next) => {
    if (next !== 'zh' && next !== 'en') return;
    setLocaleState(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // The in-memory selection still applies.
    }
  }, []);

  useEffect(() => {
    document.documentElement.lang = locale === 'zh' ? 'zh-CN' : 'en';
  }, [locale]);

  const value = useMemo(
    () => ({
      locale,
      setLocale,
      t: (text, values) => translate(locale, text, values),
    }),
    [locale, setLocale],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useTranslation() {
  const context = useContext(I18nContext);
  if (context) return context;
  return { locale: 'en', setLocale: () => {}, t: (text, values) => translate('en', text, values) };
}

export function T({ children }) {
  const { t } = useTranslation();
  return typeof children === 'string' ? t(children) : children;
}
