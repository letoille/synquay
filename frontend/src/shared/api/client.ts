export class ApiError extends Error {
  readonly status: number;
  readonly requestId: string | null;

  constructor(status: number, requestId: string | null) {
    super(`API request failed (${status})`);
    this.name = 'ApiError';
    this.status = status;
    this.requestId = requestId;
  }
}

/** Shared transport only; feature modules validate and map their own response data. */
export async function getJson(path: `/api/${string}`, signal: AbortSignal): Promise<unknown> {
  const response = await fetch(path, { signal, cache: 'no-store', headers: { Accept: 'application/json' } });
  if (!response.ok) throw new ApiError(response.status, response.headers.get('x-request-id'));
  return response.json() as Promise<unknown>;
}
