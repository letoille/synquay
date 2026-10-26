import { getJson } from '../../shared/api/client';

export async function fetchServiceStatus(signal: AbortSignal): Promise<{ version: string }> {
  const [health, info] = await Promise.all([
    getJson('/api/health', signal),
    getJson('/api/info', signal),
  ]);
  if (
    !health || typeof health !== 'object' || !('status' in health) || health.status !== 'ok' ||
    !info || typeof info !== 'object' || !('name' in info) || info.name !== 'Synquay' ||
    !('version' in info) || typeof info.version !== 'string'
  ) throw new Error('Unexpected service response');
  return { version: info.version };
}
