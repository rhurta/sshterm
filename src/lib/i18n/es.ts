import type { MessageKey } from "./en";

// Si falta una clave o sobra una, TypeScript lo marca (npm run check).
export const es: Record<MessageKey, string> = {
  "language.label": "Idioma",

  "lock.title.unlock": "Vault bloqueado",
  "lock.title.create": "Crea tu contraseña maestra",
  "lock.hint.create":
    "Protege todas las contraseñas guardadas. No hay forma de recuperarla: si la pierdes, el vault no se puede abrir.",
  "lock.field.password": "Contraseña maestra",
  "lock.field.confirm": "Repite la contraseña",
  "lock.button.unlock": "Desbloquear",
  "lock.button.create": "Crear vault",
  "lock.button.busy": "Procesando…",
  "lock.error.mismatch": "Las contraseñas no coinciden.",
  "lock.error.unexpected": "Error inesperado: {detail}",

  "error.wrong_password": "Contraseña incorrecta, o el archivo del vault está dañado.",
  "error.empty_password": "Escribe una contraseña.",
  "error.not_a_vault": "El archivo sshterm.vault no es un vault válido.",
  "error.unsupported_version": "El vault se creó con una versión más nueva de la aplicación.",
  "error.already_exists": "Ya existe un vault en esta carpeta.",
  "error.not_found": "No se encontró el archivo sshterm.vault.",
  "error.invalid_params": "El vault tiene parámetros de cifrado no válidos.",

  "home.unlocked": "Vault desbloqueado. Carpeta de datos: {dir}",
  "home.lock": "Bloquear",
};
