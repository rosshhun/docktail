// Development-only logger utility
// Only logs in development mode, silent in production

const isDev = import.meta.env.DEV;

export const logger = {
  log: (...args: any[]) => {
    if (isDev) console.log(...args);
  },
  
  warn: (...args: any[]) => {
    if (isDev) console.warn(...args);
  },
  
  error: (...args: any[]) => {
    // Always log errors, even in production
    console.error(...args);
  },
  
  debug: (...args: any[]) => {
    if (isDev) console.debug(...args);
  },
  
  info: (...args: any[]) => {
    if (isDev) console.info(...args);
  },
};
