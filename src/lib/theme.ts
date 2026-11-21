export type AppTheme = "dark" | "light" | "nord" | "latte" | "frappe" | "macchiato" | "mocha";

/** Every theme, in the order the selector lists them. */
export const THEMES: AppTheme[] = ["dark", "light", "nord", "mocha", "macchiato", "frappe", "latte"];

export interface TerminalPalette {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

interface CatppuccinFlavor {
  base: string; crust: string; surface1: string; surface2: string; text: string;
  subtext0: string; subtext1: string; rosewater: string;
  red: string; green: string; yellow: string; blue: string; pink: string; teal: string;
}

/** Official Catppuccin flavors (https://catppuccin.com/palette) mapped to the 16 ANSI colors. */
function catppuccin(c: CatppuccinFlavor): TerminalPalette {
  return {
    background: c.base, foreground: c.text, cursor: c.rosewater, cursorAccent: c.base,
    selectionBackground: c.surface2,
    black: c.surface1, red: c.red, green: c.green, yellow: c.yellow, blue: c.blue, magenta: c.pink, cyan: c.teal, white: c.subtext1,
    brightBlack: c.surface2, brightRed: c.red, brightGreen: c.green, brightYellow: c.yellow, brightBlue: c.blue, brightMagenta: c.pink, brightCyan: c.teal, brightWhite: c.subtext0,
  };
}

const palettes: Record<AppTheme, TerminalPalette> = {
  mocha: catppuccin({ base: "#1e1e2e", crust: "#11111b", surface1: "#45475a", surface2: "#585b70", text: "#cdd6f4", subtext0: "#a6adc8", subtext1: "#bac2de", rosewater: "#f5e0dc", red: "#f38ba8", green: "#a6e3a1", yellow: "#f9e2af", blue: "#89b4fa", pink: "#f5c2e7", teal: "#94e2d5" }),
  macchiato: catppuccin({ base: "#24273a", crust: "#181926", surface1: "#494d64", surface2: "#5b6078", text: "#cad3f5", subtext0: "#a5adcb", subtext1: "#b8c0e0", rosewater: "#f4dbd6", red: "#ed8796", green: "#a6da95", yellow: "#eed49f", blue: "#8aadf4", pink: "#f5bde6", teal: "#8bd5ca" }),
  frappe: catppuccin({ base: "#303446", crust: "#232634", surface1: "#51576d", surface2: "#626880", text: "#c6d0f5", subtext0: "#a5adce", subtext1: "#b5bfe2", rosewater: "#f2d5cf", red: "#e78284", green: "#a6d189", yellow: "#e5c890", blue: "#8caaee", pink: "#f4b8e4", teal: "#81c8be" }),
  latte: catppuccin({ base: "#eff1f5", crust: "#dce0e8", surface1: "#bcc0cc", surface2: "#acb0be", text: "#4c4f69", subtext0: "#6c6f85", subtext1: "#5c5f77", rosewater: "#dc8a78", red: "#d20f39", green: "#40a02b", yellow: "#df8e1d", blue: "#1e66f5", pink: "#ea76cb", teal: "#179299" }),
  dark: {
    background: "#161a22",
    foreground: "#dfe4ec",
    cursor: "#8fb4e8",
    cursorAccent: "#161a22",
    selectionBackground: "#3c4b62",
    black: "#1a1f29",
    red: "#f08a7a",
    green: "#78bfa5",
    yellow: "#e3b341",
    blue: "#8fb4e8",
    magenta: "#c49bd2",
    cyan: "#75c6d1",
    white: "#dfe4ec",
    brightBlack: "#788397",
    brightRed: "#ff9c8f",
    brightGreen: "#91d6b8",
    brightYellow: "#f3ca6b",
    brightBlue: "#aac9f2",
    brightMagenta: "#d8b1e3",
    brightCyan: "#8ad9e2",
    brightWhite: "#ffffff",
  },
  light: {
    background: "#ffffff",
    foreground: "#243247",
    cursor: "#356bb3",
    cursorAccent: "#ffffff",
    selectionBackground: "#c8daf1",
    black: "#263445",
    red: "#bd3c45",
    green: "#267a52",
    yellow: "#946200",
    blue: "#356bb3",
    magenta: "#8752a1",
    cyan: "#1e7f8e",
    white: "#e5eaf1",
    brightBlack: "#69788e",
    brightRed: "#d64c55",
    brightGreen: "#358b62",
    brightYellow: "#ad780d",
    brightBlue: "#4c82c8",
    brightMagenta: "#9b68b5",
    brightCyan: "#2c98a8",
    brightWhite: "#ffffff",
  },
  nord: {
    background: "#2e3440",
    foreground: "#eceff4",
    cursor: "#88c0d0",
    cursorAccent: "#2e3440",
    selectionBackground: "#4c566a",
    black: "#3b4252",
    red: "#bf616a",
    green: "#a3be8c",
    yellow: "#ebcb8b",
    blue: "#81a1c1",
    magenta: "#b48ead",
    cyan: "#88c0d0",
    white: "#e5e9f0",
    brightBlack: "#616e84",
    brightRed: "#d57780",
    brightGreen: "#b7d29f",
    brightYellow: "#f2d59a",
    brightBlue: "#8fb0d0",
    brightMagenta: "#c69fbe",
    brightCyan: "#9bd0df",
    brightWhite: "#eceff4",
  },
};

/** `#rrggbb` + percent opacity -> `rgba(...)`; 100 keeps the plain hex color. */
export function withOpacity(hex: string, percent: number): string {
  if (percent >= 100 || !/^#[0-9a-f]{6}$/i.test(hex)) return hex;
  const n = parseInt(hex.slice(1), 16);
  const alpha = Math.max(0, Math.min(100, percent)) / 100;
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

/**
 * Terminal colors for a theme. `opacity` (percent) makes the terminal background
 * translucent so a window background image shows through; xterm needs
 * `allowTransparency` for that.
 */
export function getTerminalTheme(theme: AppTheme, opacity = 100): TerminalPalette {
  const palette = palettes[theme] ?? palettes.dark;
  return opacity >= 100 ? palette : { ...palette, background: withOpacity(palette.background, opacity) };
}
