/** Stop waiting promptly while retaining rejection handlers on uncancelable browser work. */
export function abortable<T>(
  work: Promise<T>,
  signal?: AbortSignal,
): Promise<T> {
  if (!signal) return work;
  return new Promise<T>((resolve, reject) => {
    const abort = () => {
      cleanup();
      reject(signal.reason ?? new DOMException("Aborted", "AbortError"));
    };
    const cleanup = () => signal.removeEventListener("abort", abort);
    work.then(
      (value) => {
        cleanup();
        if (signal.aborted) abort();
        else resolve(value);
      },
      (reason) => {
        cleanup();
        reject(reason);
      },
    );
    signal.addEventListener("abort", abort, { once: true });
    if (signal.aborted) abort();
  });
}
