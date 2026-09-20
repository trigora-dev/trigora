import pc from 'picocolors';

export const colors = {
  id: pc.cyan,
  dev: pc.dim,
  label: pc.dim,
  heading: pc.bold,
  link: (value: string) => pc.cyan(pc.underline(value)),

  info: pc.blue, // INFO → informational
  warn: pc.yellow, // WARN → attention
  error: pc.red, // ERROR → critical
  success: pc.green, // succeeded → positive outcome
};
