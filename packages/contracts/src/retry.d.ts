/**
 * Backoff strategy for flow retries.
 *
 * Currently only exponential backoff is supported.
 */
export type RetryBackoff = 'exponential';
/**
 * Retry policy for a flow definition.
 *
 * When omitted on a flow, the effective policy is `{ attempts: 1, backoff: 'exponential' }`.
 * `attempts > 1` is queue-only; webhook, cron, and manual flows may only use `attempts: 1`.
 */
export type RetryPolicy = {
  /**
   * Maximum delivery attempts, inclusive. Integer from 1 to 20.
   */
  attempts: number;
  backoff: RetryBackoff;
};
