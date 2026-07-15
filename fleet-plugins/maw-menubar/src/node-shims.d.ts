declare module "fs" {
  export function existsSync(path: string): boolean;
  export function mkdirSync(path: string, options: { recursive: boolean; mode?: number }): void;
  export function renameSync(from: string, to: string): void;
  export function rmSync(path: string, options: { force: boolean }): void;
  export function writeFileSync(path: string, data: string, options: { mode: number }): void;
}

declare module "path" {
  export function dirname(path: string): string;
  export function isAbsolute(path: string): boolean;
  export function join(...parts: string[]): string;
}
