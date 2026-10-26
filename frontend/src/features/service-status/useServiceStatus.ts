import { useEffect, useState } from 'react';
import { ApiError } from '../../shared/api/client';
import { fetchServiceStatus } from './api';

type ServiceState =
  | { status: 'loading' }
  | { status: 'online'; version: string }
  | { status: 'offline'; requestId: string | null };

export function useServiceStatus() {
  const [service, setService] = useState<ServiceState>({ status: 'loading' });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 5000);
    let active = true;
    setService({ status: 'loading' });
    void fetchServiceStatus(controller.signal)
      .then(({ version }) => {
        if (active) setService({ status: 'online', version });
      })
      .catch((error: unknown) => {
        if (active) setService({ status: 'offline', requestId: error instanceof ApiError ? error.requestId : null });
      })
      .finally(() => window.clearTimeout(timeout));
    return () => {
      active = false;
      window.clearTimeout(timeout);
      controller.abort();
    };
  }, [attempt]);

  return { service, retry: () => setAttempt((value) => value + 1) };
}
