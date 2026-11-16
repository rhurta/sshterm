export type AppTheme = "dark" | "light" | "nord";

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

const palettes: Record<AppTheme, TerminalPalette> = {
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

export function getTerminalTheme(theme: AppTheme): TerminalPalette {
  return palettes[theme] ?? palettes.dark;
}
