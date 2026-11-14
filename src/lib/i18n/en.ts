// English is the default language and the source of truth for message keys.
// Every other language must provide exactly these keys (checked by TypeScript).
export const en = {
  "language.label": "Language",

  "lock.title.unlock": "Vault locked",
  "lock.title.create": "Create your master password",
  "lock.hint.create":
    "It protects every saved password. There is no way to recover it: if you lose it, the vault cannot be opened.",
  "lock.field.password": "Master password",
  "lock.field.confirm": "Repeat the password",
  "lock.button.unlock": "Unlock",
  "lock.button.create": "Create vault",
  "lock.button.busy": "Working…",
  "lock.error.mismatch": "The passwords do not match.",
  "lock.error.unexpected": "Unexpected error: {detail}",

  "error.wrong_password": "Wrong password, or the vault file is damaged.",
  "error.empty_password": "Enter a password.",
  "error.not_a_vault": "The file sshterm.vault is not a valid vault.",
  "error.unsupported_version": "The vault was created by a newer version of the application.",
  "error.already_exists": "A vault already exists in this folder.",
  "error.not_found": "The file sshterm.vault was not found.",
  "error.invalid_params": "The vault has invalid encryption parameters.",

  "home.unlocked": "Vault unlocked. Data folder: {dir}",
  "home.lock": "Lock",
} as const;

export type MessageKey = keyof typeof en;
