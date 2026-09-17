import { type ClassValue, clsx } from 'clsx';

/**
 * Utility for composing class names with conditional logic
 * Similar to Tailwind's cn() utility
 */
export function cn(...inputs: ClassValue[]) {
  return clsx(inputs);
}

/**
 * Glass morphism effect styles
 * Returns a string of CSS classes for glass effect
 */
export function glassStyle(theme: 'light' | 'dark' = 'dark'): string {
  if (theme === 'dark') {
    return 'backdrop-blur-xl bg-[#161615]/70 border border-[#2c2c2a]/50 shadow-lg shadow-black/20';
  }
  return 'backdrop-blur-xl bg-[#ffffff]/70 border border-[#e1e0d9]/50 shadow-lg shadow-black/10';
}

/**
 * Generate a smooth gradient for accents
 */
export function gradientAccent(): string {
  return 'bg-gradient-to-br from-[#2a78d6] to-[#4a3aa7]';
}

/**
 * Animation variants for framer-motion
 */
export const fadeVariants = {
  hidden: { opacity: 0 },
  visible: { opacity: 1, transition: { duration: 0.2 } },
  exit: { opacity: 0, transition: { duration: 0.15 } },
};

export const slideVariants = {
  hidden: { opacity: 0, x: -20 },
  visible: { opacity: 1, x: 0, transition: { duration: 0.2 } },
  exit: { opacity: 0, x: 20, transition: { duration: 0.15 } },
};

export const scaleVariants = {
  hidden: { opacity: 0, scale: 0.95 },
  visible: { opacity: 1, scale: 1, transition: { duration: 0.2 } },
  exit: { opacity: 0, scale: 0.95, transition: { duration: 0.15 } },
};
