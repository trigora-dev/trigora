import pc from 'picocolors';

export const colors = {
  flow: pc.cyan, // [flow.id] → stands out (primary signal)
  dev: pc.dim, // [dev] → subtle, background system logs
  label: pc.dim,
  heading: pc.bold,

  run: pc.dim, // RUN → low emphasis label
  info: pc.blue, // INFO → informational
  warn: pc.yellow, // WARN → attention
  error: pc.red, // ERROR → critical
  success: pc.green, // succeeded → positive outcome
};
