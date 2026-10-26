/** Use the numeric loopback for generated local endpoints; retain deployment origins. */
export function generatedOrigin(origin: string = window.location.origin): string {
  const url = new URL(origin);
  if (url.hostname === 'localhost') url.hostname = '127.0.0.1';
  return url.origin;
}
