import pc from 'picocolors';

export const colors = {
  flow: pc.cyan, // [flow.id] → stands out (primary signal)
  dev: pc.dim, // subtle progress or background runtime notes
  label: pc.dim,
  heading: pc.bold,
  link: (value: string) => pc.cyan(pc.underline(value)),

  info: pc.blue, // INFO → informational
  warn: pc.yellow, // WARN → attention
  error: pc.red, // ERROR → critical
  success: pc.green, // succeeded → positive outcome
};
