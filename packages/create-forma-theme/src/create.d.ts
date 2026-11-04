/** Types for create.js, which stays plain JavaScript so npx runs it without a build. */
export declare const SDK_VERSION: string;
export declare function toId(name: string): string;
export declare function createTheme(
  dir: string,
  options?: { name?: string; id?: string; author?: string }
): { dir: string; id: string; name: string; author: string };
