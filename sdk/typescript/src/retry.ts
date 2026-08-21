// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Retry policy — Phase API-4A §8.
//
// Same three rules as the Python SDK:
//   1. Retry lives only in the handwritten layer.
//   2. Nothing is retried blindly. Safe methods always; writes only when they
//      carry an idempotency signal the node dedups on.
//   3. `Retry-After` wins over computed backoff.
//
// It is implemented as a `fetch` wrapper handed to the generated HttpClient via
// its `customFetch` hook, so retries happen underneath the generated client —
// no second HTTP stack.

/** Header the SDK sets from a caller-supplied `requestId`. */
export const IDEMPOTENCY_HEADER = "Idempotency-Key";

export interface RetryPolicy {
  /** Total attempts including the first. `1` disables retry entirely. */
  maxAttempts: number;
  /** First backoff delay, in milliseconds. */
  backoffInitialMs: number;
  /** Multiplier applied after each attempt. */
  backoffMultiplier: number;
  /** Ceiling on a single backoff delay, in milliseconds. */
  backoffMaxMs: number;
  /** Full-jitter fraction, 0–1. `0` makes delays deterministic. */
  jitter: number;
  /** Statuses worth trying again. */
  retryStatus: number[];
  /** Methods safe to repeat with no further evidence. */
  safeMethods: string[];
  /** Retry network-level failures for eligible requests. */
  retryOnNetworkError: boolean;
  /** When false, a write is never retried even with an idempotency key. */
  retryIdempotentWrites: boolean;
  /** Honour `Retry-After` in preference to computed backoff. */
  respectRetryAfter: boolean;
  /** Upper bound on a server-named `Retry-After`, in milliseconds. */
  retryAfterMaxMs: number;
}

export const DEFAULT_RETRY_POLICY: RetryPolicy = {
  maxAttempts: 3,
  backoffInitialMs: 250,
  backoffMultiplier: 2,
  backoffMaxMs: 8000,
  jitter: 0.1,
  retryStatus: [408, 429, 500, 502, 503, 504],
  safeMethods: ["GET", "HEAD", "OPTIONS"],
  retryOnNetworkError: true,
  retryIdempotentWrites: true,
  respectRetryAfter: true,
  retryAfterMaxMs: 60_000,
};

export function resolveRetryPolicy(partial?: Partial<RetryPolicy>): RetryPolicy {
  return { ...DEFAULT_RETRY_POLICY, ...(partial ?? {}) };
}

/** Is this request shape eligible for a second attempt at all? */
export function isRetryableRequest(
  policy: RetryPolicy,
  method: string,
  hasIdempotencyKey: boolean,
): boolean {
  if (policy.maxAttempts <= 1) return false;
  if (policy.safeMethods.includes(method.toUpperCase())) return true;
  return policy.retryIdempotentWrites && hasIdempotencyKey;
}

export function shouldRetryStatus(policy: RetryPolicy, status: number): boolean {
  return policy.retryStatus.includes(status);
}

/** Milliseconds to wait before attempt `attempt + 1` (`attempt` is 1-based). */
export function delayFor(
  policy: RetryPolicy,
  attempt: number,
  retryAfterSeconds?: number,
  random: () => number = Math.random,
): number {
  if (policy.respectRetryAfter && retryAfterSeconds !== undefined) {
    return Math.max(0, Math.min(retryAfterSeconds * 1000, policy.retryAfterMaxMs));
  }
  let base = policy.backoffInitialMs * policy.backoffMultiplier ** (attempt - 1);
  base = Math.min(base, policy.backoffMaxMs);
  if (policy.jitter) base += base * policy.jitter * random();
  return base;
}

function retryAfterSeconds(response: Response): number | undefined {
  const raw = response.headers.get("retry-after");
  if (!raw) return undefined;
  const seconds = Number(raw);
  return Number.isFinite(seconds) ? seconds : undefined;
}

export interface RetryFetchOptions {
  policy: RetryPolicy;
  fetch: typeof fetch;
  /** Injectable so tests never actually wait. */
  sleep?: (ms: number) => Promise<void>;
  random?: () => number;
}

const defaultSleep = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

/**
 * Wrap a `fetch` implementation with the retry policy.
 *
 * The returned function has the same signature as `fetch`, so the generated
 * HttpClient can take it as `customFetch` and stay a single-shot caller.
 */
export function withRetry(options: RetryFetchOptions): typeof fetch {
  const { policy, fetch: inner } = options;
  const sleep = options.sleep ?? defaultSleep;
  const random = options.random ?? Math.random;

  return async function retryingFetch(
    input: RequestInfo | URL,
    init?: RequestInit,
  ): Promise<Response> {
    const method = (init?.method ?? "GET").toUpperCase();
    const headers = new Headers(init?.headers ?? {});
    const eligible = isRetryableRequest(policy, method, headers.has(IDEMPOTENCY_HEADER));

    let attempt = 0;
    // eslint-disable-next-line no-constant-condition
    while (true) {
      attempt += 1;
      let response: Response;
      try {
        response = await inner(input, init);
      } catch (cause) {
        if (!eligible || !policy.retryOnNetworkError || attempt >= policy.maxAttempts) {
          throw cause;
        }
        await sleep(delayFor(policy, attempt, undefined, random));
        continue;
      }

      if (
        !eligible ||
        attempt >= policy.maxAttempts ||
        !shouldRetryStatus(policy, response.status)
      ) {
        return response;
      }
      await sleep(delayFor(policy, attempt, retryAfterSeconds(response), random));
    }
  };
}
