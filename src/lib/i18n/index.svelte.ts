import { invoke } from "@tauri-apps/api/core";
import { en, type MessageKey } from "./en";
import { es } from "./es";

export type { MessageKey };

export const DEFAULT_LOCALE = "en";

// To add a language: 1) copy en.ts to xx.ts and translate the values,
// 2) import it here, 3) add one line to `catalogs` and one to `locales`.
const catalogs: Record<string, Record<MessageKey, string>> = { en, es };

/** Names are written in their own language so anyone can find theirs. */
export const locales = [
  { code: "en", name: "English" },
  { code: "es", name: "Español" },
];

let current = $state(DEFAULT_LOCALE);

export const i18n = {
  get locale() {
    return current;
  },
};

export function hasMessage(key: string): key is MessageKey {
  return Object.hasOwn(en, key);
}

/** Translate a key. Missing translations fall back to English. `{name}` placeholders are replaced. */
export function t(key: MessageKey, params: Record<string, string | number> = {}): string {
  const template = catalogs[current]?.[key] ?? en[key];
  return template.replace(/\{(\w+)\}/g, (_, name: string) => String(params[name] ?? `{${name}}`));
}

/** Reads the saved language from config.toml. Call once at startup. */
export async function initLocale(): Promise<void> {
  try {
    const saved = await invoke<string>("config_get_language");
    if (Object.hasOwn(catalogs, saved)) current = saved;
  } catch {
    // keep the default language
  }
  document.documentElement.lang = current;
}

/** Changes the language immediately and saves it to config.toml. */
export async function setLocale(code: string): Promise<void> {
  if (!Object.hasOwn(catalogs, code)) return;
  current = code;
  document.documentElement.lang = code;
  try {
    await invoke("config_set_language", { language: code });
  } catch {
    // the language still changes for this session
  }
}
